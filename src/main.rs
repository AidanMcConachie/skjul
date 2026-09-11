use axum::{
    extract::Form,
    http::header,
    response::Html,
    routing::{get, post},
    Router,
};

use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    text: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/styles.css", get(styles))
        .route("/transform", post(transform));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn styles() -> ([(header::HeaderName, &'static str); 1], &'static str) {
    (
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("static/styles.css"),
    )
}

async fn transform(Form(input): Form<Input>) -> Html<String> {
    let result = input.text.replace(" ", ""); // exit point for obfuscated code

    Html(format!("{}", result))
}
