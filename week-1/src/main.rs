use hyper::{
    Body, Method, Request, Response, Server, StatusCode,
    service::{make_service_fn, service_fn},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Book {
    id: u64,
    title: String,
    author: String,
    isbn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateBookRequest {
    title: String,
    author: String,
    isbn: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateBookRequest {
    title: Option<String>,
    author: Option<String>,
    isbn: Option<String>,
}

struct Storage {
    books: HashMap<u64, Book>,
    next_id: u64,
}

impl Storage {
    fn new() -> Self {
        Storage {
            books: HashMap::new(),
            next_id: 1,
        }
    }
}

type SharedState = Arc<Mutex<Storage>>;

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let state = Arc::new(Mutex::new(Storage::new()));

    let service = make_service_fn(move |_| {
        let state = state.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| handle_request(req, state.clone())))
        }
    });

    let server = Server::bind(&addr).serve(service);
    println!("Server running on http://{}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

async fn handle_request(
    req: Request<Body>,
    state: SharedState,
) -> Result<Response<Body>, hyper::Error> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    match (method, path.as_str()) {
        (Method::POST, "/books") => create_book(req, state).await,
        (Method::GET, "/books") => get_all_books(state).await,
        (Method::GET, path) if path.starts_with("/books/") => handle_book_id(path, state, |id| get_book(id, state)).await,
        (Method::PUT, path) if path.starts_with("/books/") => handle_book_id(path, state, |id| update_book(id, req, state)).await,
        (Method::DELETE, path) if path.starts_with("/books/") => handle_book_id(path, state, |id| delete_book(id, state)).await,
        _ => Ok(not_found()),
    }
}

async fn handle_book_id<F, Fut>(
    path: &str,
    state: SharedState,
    handler: F,
) -> Result<Response<Body>, hyper::Error>
where
    F: FnOnce(u64) -> Fut,
    Fut: std::future::Future<Output = Result<Response<Body>, hyper::Error>>,
{
    match path.trim_start_matches("/books/").parse::<u64>() {
        Ok(id) => handler(id).await,
        Err(_) => Ok(bad_request("Invalid book ID")),
    }
}

async fn create_book(
    req: Request<Body>,
    state: SharedState,
) -> Result<Response<Body>, hyper::Error> {
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    let create_req: Result<CreateBookRequest, _> = serde_json::from_slice(&body_bytes);

    match create_req {
        Ok(create_req) => {
            let mut storage = state.lock().await;
            let id = storage.next_id;
            storage.next_id += 1;
            
            let book = Book {
                id,
                title: create_req.title,
                author: create_req.author,
                isbn: create_req.isbn,
            };
            
            storage.books.insert(id, book.clone());
            json_response(StatusCode::CREATED, &book)
        }
        Err(_) => Ok(bad_request("Invalid request body")),
    }
}

async fn get_all_books(state: SharedState) -> Result<Response<Body>, hyper::Error> {
    let storage = state.lock().await;
    let books: Vec<Book> = storage.books.values().cloned().collect();
    json_response(StatusCode::OK, &books)
}

async fn get_book(id: u64, state: SharedState) -> Result<Response<Body>, hyper::Error> {
    let storage = state.lock().await;
    match storage.books.get(&id) {
        Some(book) => json_response(StatusCode::OK, book),
        None => Ok(not_found()),
    }
}

async fn update_book(
    id: u64,
    req: Request<Body>,
    state: SharedState,
) -> Result<Response<Body>, hyper::Error> {
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    let update_req: Result<UpdateBookRequest, _> = serde_json::from_slice(&body_bytes);

    match update_req {
        Ok(update_req) => {
            let mut storage = state.lock().await;
            match storage.books.get_mut(&id) {
                Some(book) => {
                    if let Some(title) = update_req.title {
                        book.title = title;
                    }
                    if let Some(author) = update_req.author {
                        book.author = author;
                    }
                    if let Some(isbn) = update_req.isbn {
                        book.isbn = Some(isbn);
                    }
                    json_response(StatusCode::OK, book)
                }
                None => Ok(not_found()),
            }
        }
        Err(_) => Ok(bad_request("Invalid request body")),
    }
}

async fn delete_book(id: u64, state: SharedState) -> Result<Response<Body>, hyper::Error> {
    let mut storage = state.lock().await;
    if storage.books.remove(&id).is_some() {
        Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap())
    } else {
        Ok(not_found())
    }
}

fn json_response<T: Serialize>(
    status: StatusCode,
    data: &T,
) -> Result<Response<Body>, hyper::Error> {
    match serde_json::to_string(data) {
        Ok(body) => Ok(Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap()),
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Error serializing response"))
            .unwrap()),
    }
}

fn bad_request(message: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::from(message))
        .unwrap()
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not found"))
        .unwrap()
}