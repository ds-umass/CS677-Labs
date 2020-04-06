#[macro_use]
extern crate diesel;
#[macro_use]
extern crate lazy_static;
extern crate dotenv;

mod data;
mod models;
mod schema;
mod configs;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use dotenv::dotenv;
use std::env;
use actix_web::{web, App, HttpRequest, HttpServer, Responder, HttpResponse};
use actix_web::web::Json;
use crate::data::establish_connection;
use crate::models::{LookupRes, Item, Topic};
use crate::configs::*;
use std::collections::HashMap;
use actix_web::error::ParseError::TooLarge;
use actix_web::middleware::Logger;
use log::*;

lazy_static! {
    // Pre-initialize topics from database
    static ref TOPICS: HashMap<i32, Topic> = {
        use schema::topic::dsl::*;
        topic.load::<Topic>(&establish_connection())
        .unwrap()
        .into_iter()
        // take its id as key, topic content as value
        .map(|t| (t.id, t))
        .collect()
    };
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    // Initialize configure reader
    dotenv().ok();
    // Initialize logger
    simple_logger::init().unwrap();
    HttpServer::new(||
        {
            App::new()
                // Setup logging middleware for HTTP server
                .wrap(Logger::default())
                // Search route. Topic is a string that does not require exact matching
                .route("/search/{topic}", web::get().to(search_handler))
                // Lookup an item with exact item id
                .route("/lookup/{id}", web::get().to(lookup_handler))
                // List all items in the database
                .route("/lookup", web::get().to(list_all))
                // Update item stock
                .route("/update/{id}/stock/deduct/{stock}", web::post().to(update_stock))
        })
        // Set binding port
        .bind(format!("0.0.0.0:{}", *CAT_SERVER_PORT))?
        // Run the server
        .run()
        .await
}

async fn search_handler(req: HttpRequest) -> impl Responder {
    use schema::item::dsl::*;
    // Get topic string
    let topic_query = req.match_info().get("topic").unwrap_or("");
    // Extract topics matches query
    let topic_matched = TOPICS.values().filter_map(|matching_topic| {
        // By checking the topic lowercase string contains searching string in lowercase
        if matching_topic.name.to_lowercase().contains(&topic_query.to_lowercase()) {
            Some(matching_topic)
        } else {
            None
        }
    }).collect::<Vec<&Topic>>();
    // Query by SQL statement, finding items in the matching topics
    // The ORM we are using diesel does not support IN statement for query, we compile it by ourselves
    let items = diesel::sql_query(format!(
        "SELECT * FROM item WHERE topic IN ({})",
        topic_matched.iter().map(|i| format!("{}", i.id)).collect::<Vec<_>>().join(", ")
    )).load::<Item>(&establish_connection()).unwrap();
    // Compose a structure indicates the status of the result
    let mut res = LookupRes::from_lookup::<()>(Ok(items));
    // Provide topic name and it as one of the field in result because topic in item is topic id
    res.topics = topic_matched.into_iter().cloned().collect();
    // Return the result to client in Json
    HttpResponse::Ok().json(res)
}

async fn lookup_handler(req: HttpRequest) -> impl Responder {
    use schema::item::dsl::*;
    // Get item it from url
    let item_id: i32 = req.match_info().get("id").unwrap().parse().unwrap();
    let mut res = LookupRes::from_lookup(
        // Get the item from database by its id
        // Using diesel query DSL
        item
        .filter(id.eq(item_id))
        .get_result::<Item>(&establish_connection())
    );
    // Check if we can find the item. If yes, attach the topic information
    if res.ok {
        res.topics = vec![TOPICS[&res.result.as_ref().unwrap().topic].clone()]
    }
    // Return the result
    HttpResponse::Ok().json(res)
}

async fn list_all(req: HttpRequest) -> impl Responder {
    use schema::item::dsl::*;
    // Get all items
    let all = item.load::<Item>(&establish_connection());
    let mut res = LookupRes::from_lookup(all);
    // Attach all topics
    res.topics = TOPICS.values().cloned().collect();
    HttpResponse::Ok().json(res)
}

async fn update_stock(req: HttpRequest) -> impl Responder {
    use schema::item::dsl::*;
    let item_id: i32 = req.match_info().get("id").unwrap().parse().unwrap();
    let stock_deduct: i32 = req.match_info().get("stock").unwrap().parse().unwrap();
    // Get connection
    let conn = establish_connection();
    // Run a transaction.
    // In this transaction, we need to check if there are enough stock for the transaction.
    // If not enough stock, do nothing and return false
    let txn_res: Result<_, diesel::result::Error> = conn.transaction(|| {
        // Get the item entity
        if let Ok(i) = item.filter(id.eq(item_id)).get_result::<Item>(&conn) {
            // Only update when there are enough stock to deduct
            if i.stock >= stock_deduct {
                // Update stock number for the item
                diesel::update(item)
                    .filter(id.eq(item_id))
                    .set(stock.eq(i.stock - stock_deduct))
                    .execute(&conn);
                return Ok(true);
            }
        }
        Ok(false)
    });
    // Return the transaction result
    HttpResponse::Ok().json(txn_res.unwrap())
}


