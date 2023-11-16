use futures::StreamExt;
use std::str::FromStr;
use tauri::Manager;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Method, Request, Response, Server, StatusCode};

use tracing::error;

use crate::state::SharedState;
use crate::store::{InProgressStream, MimeType};
use crate::ui::generate_preview;
use crate::view;

async fn handle(
    req: Request<Body>,
    state: SharedState,
    app_handle: tauri::AppHandle,
) -> Result<Response<Body>, Error> {
    let path = req.uri().path();
    let id = path
        .strip_prefix("/")
        .and_then(|id| scru128::Scru128Id::from_str(id).ok());

    match (req.method(), id) {
        (&Method::GET, Some(id)) => {
            let accept_header = req
                .headers()
                .get(hyper::header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("text/plain");

            get(state, id, accept_header).await
        }
        (&Method::POST, None) if path == "/" => post(req, state.clone(), app_handle.clone()).await,
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap()),
    }
}

#[tracing::instrument(skip_all)]
async fn get_stack(
    state: SharedState,
    item: view::Item,
    accept: &str,
) -> Result<Response<Body>, Error> {
    let items: Vec<_> = state.with_lock(|state| {
        state
            .view
            .children(&item)
            .iter()
            .map(|id| {
                let item = state.view.items.get(id).unwrap();
                (
                    item.clone(),
                    state.store.get_content_meta(&item.hash).unwrap(),
                    state.store.get_content(&item.hash).unwrap(),
                )
            })
            .collect()
    });
    let accept = accept.to_string();
    let (mut tx, rx) = hyper::Body::channel();
    tokio::spawn(async move {
        for (_item, meta, mut content) in items {
            if accept == "text/html" {
                content = generate_preview(
                    "light",
                    &Some(content),
                    &meta.mime_type,
                    &meta.content_type,
                    false,
                )
                .into_bytes();
            }
            tx.send_data(content.into()).await.unwrap();
            tx.send_data("\n".into()).await.unwrap();
        }
    });
    Ok(Response::builder().status(StatusCode::OK).body(rx).unwrap())
}

#[tracing::instrument(skip(state))]
async fn get(
    state: SharedState,
    id: scru128::Scru128Id,
    accept: &str,
) -> Result<Response<Body>, Error> {
    let item = state.with_lock(|state| state.view.items.get(&id).cloned());

    match item {
        Some(item) => {
            if item.stack_id.is_none() {
                return get_stack(state, item, accept).await;
            }

            let cache_path = state.with_lock(|state| state.store.cache_path.clone());
            let reader = cacache::Reader::open_hash(cache_path, item.hash)
                .await
                .unwrap();
            let stream = Body::wrap_stream(tokio_util::io::ReaderStream::new(reader));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(stream)
                .unwrap())
        }
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap()),
    }
}

async fn post(
    req: Request<Body>,
    state: SharedState,
    app_handle: tauri::AppHandle,
) -> Result<Response<Body>, Error> {
    let mut streamer = state.with_lock(|state| {
        let stack = state.get_curr_stack();
        state.ui.select(None); // focus first
        let streamer = InProgressStream::new(Some(stack), "".as_bytes());
        state.merge(&streamer.packet);
        app_handle.emit_all("refresh-items", true).unwrap();
        streamer
    });

    let mut bytes_stream = req.into_body();

    #[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq)]
    pub struct Content {
        pub mime_type: MimeType,
        pub content_type: String,
        pub terse: String,
        pub tiktokens: usize,
        pub words: usize,
        pub chars: usize,
        pub preview: String,
    }

    while let Some(chunk) = bytes_stream.next().await {
        match chunk {
            Ok(chunk) => {
                streamer.append(&chunk);
                let preview = generate_preview(
                    "dark",
                    &Some(streamer.content.clone()),
                    &MimeType::TextPlain,
                    &"Text".to_string(),
                    true,
                );

                let content = String::from_utf8_lossy(&streamer.content);
                let content = Content {
                    mime_type: MimeType::TextPlain,
                    content_type: "Text".to_string(),
                    terse: content.chars().take(100).collect(),
                    tiktokens: 0,
                    words: content.split_whitespace().count(),
                    chars: content.chars().count(),
                    preview,
                };

                app_handle
                    .emit_all("streaming", (streamer.packet.id, content))
                    .unwrap();
            }
            Err(e) => {
                tracing::error!("Error reading bytes from HTTP POST: {}", e);
            }
        }
    }

    state.with_lock(|state| {
        let packet = streamer.end_stream(&mut state.store);
        state.merge(&packet);
        state.store.insert_packet(&packet);
    });
    app_handle.emit_all("refresh-items", true).unwrap();

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(streamer.packet.id.to_string()))
        .unwrap())
}

pub fn start(app_handle: tauri::AppHandle, state: SharedState) {
    tauri::async_runtime::spawn(async move {
        let addr = ([127, 0, 0, 1], 9146).into();

        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            let app_handle = app_handle.clone();
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                    handle(req, state.clone(), app_handle.clone())
                }))
            }
        });

        let server = Server::bind(&addr).serve(make_svc);

        if let Err(e) = server.await {
            error!("server error: {}", e);
        }
    });
}
