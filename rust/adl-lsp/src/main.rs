use std::ops::ControlFlow;
use std::time::Duration;

use async_lsp::client_monitor::ClientProcessMonitorLayer;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use clap::Parser;
use lsp_types::{notification, request};
use tower::ServiceBuilder;
use tracing::{Level, debug, trace};

use crate::cli::Cli;
use crate::server::Server;

mod cli;
mod node;
mod parser;
mod server;

struct TickEvent;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();

    let (server, _) = async_lsp::MainLoop::new_server(|client| {
        tokio::spawn({
            let client = client.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    if client.emit(TickEvent).is_err() {
                        break;
                    }
                }
            }
        });

        let mut router: Router<Server> = Server::new(&client, (&cli).into()).into();

        router
            .request::<request::Initialize, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_initialize(params).await }
            })
            .request::<request::Shutdown, _>(|st, _| {
                let st = st.clone();
                async move { st.handle_shutdown().await }
            })
            .request::<request::HoverRequest, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_hover_request(params).await }
            })
            .request::<request::GotoDefinition, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_goto_definition(params) }
            })
            .request::<request::References, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_find_references(params) }
            })
            .request::<request::DocumentDiagnosticRequest, _>(|st, params| {
                let st = st.clone();
                async move { st.handle_document_diagnostic_request(params) }
            })
            .request::<request::DocumentSymbolRequest, _>(|st, params| {
                let mut st = st.clone();
                async move { st.handle_document_symbol_request(params) }
            })
            .notification::<notification::DidOpenTextDocument>(|st, params| {
                trace!("did open text document: {:?}", params);
                st.handle_did_open_text_document(params)
            })
            .notification::<notification::DidChangeTextDocument>(|st, params| {
                trace!("did change text document: {:?}", params);
                st.handle_did_change_text_document(params)
            })
            .notification::<notification::DidSaveTextDocument>(|st, params| {
                trace!("did save text document: {:?}", params);
                st.handle_did_save_text_document(params)
            })
            .notification::<notification::DidCloseTextDocument>(|st, params| {
                trace!("did close text document: {:?}", params);
                st.handle_did_close_text_document(params)
            })
            .notification::<notification::Exit>(|st, _| st.handle_exit())
            // TODO: handle these notifications
            .notification::<notification::Initialized>(|_, _| ControlFlow::Continue(()))
            .notification::<notification::DidChangeConfiguration>(|_, _| ControlFlow::Continue(()))
            .event::<TickEvent>(|st, _| st.handle_tick_event());

        ServiceBuilder::new()
            .layer(TracingLayer::default())
            .layer(LifecycleLayer::default())
            .layer(CatchUnwindLayer::default())
            .layer(ConcurrencyLayer::default())
            .layer(ClientProcessMonitorLayer::new(client))
            .service(router)
    });

    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    debug!("cli: {:?}", cli);

    // Prefer truly asynchronous piped stdin/stdout without blocking tasks.
    #[cfg(unix)]
    let (stdin, stdout) = (
        async_lsp::stdio::PipeStdin::lock_tokio().unwrap(),
        async_lsp::stdio::PipeStdout::lock_tokio().unwrap(),
    );
    // Fallback to spawn blocking read/write otherwise.
    #[cfg(not(unix))]
    let (stdin, stdout) = (
        tokio_util::compat::TokioAsyncReadCompatExt::compat(tokio::io::stdin()),
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tokio::io::stdout()),
    );

    server.run_buffered(stdin, stdout).await.unwrap();
}
