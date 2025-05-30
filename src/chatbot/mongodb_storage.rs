use std::env;

use actix_web::HttpResponse;
use futures::TryStreamExt;
use mongodb::{bson::doc, Database};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use crate::{auth::get_mongodb_uri, chatbot::{thread_storage::cleanup_conversation, types}};

/// Stores and loads threads from the mongoDB
use super::types::Conversation;

// Note: Bianca needs the user_id, thread_id, date and "topic" of a thread for the frontend, so that will be the four contents beside the main content.
/// The content of a thread in the mongoDB database.
#[derive(Debug, Deserialize, Serialize)]
pub struct MongoDBThread {
    pub user_id: String,
    pub thread_id: String,
    pub date: String, // ISO 8601 date
    pub topic: String, // The first message in the thread, for now. Later maybe a summary of the thread.
    pub content: Conversation,
}


/// Stores a thread in the mongoDB database, appending the content if the thread already exists.
pub async fn append_thread(thread_id: &str, user_id: &str, content: Conversation, database: Database) {
    debug!("Will append content to thread {} for user {}", thread_id, user_id);
    trace!("Content: {:?}", content);
    let mut content = content;
    cleanup_conversation(&mut content);
    trace!("Cleaned content: {:?}", content);

    if content.is_empty() {
        debug!("Content is empty, will not append to thread.");
        return;
    }

    // We first need to retrieve the thread from the database, if it exists.
    let existing_thread = read_thread(thread_id, database.clone()).await;

    // If there is some existing thread, we need to update the content.
    // The new content is the old content + the new content.
    let (content, thread_exists) = match existing_thread {
        Some(existing_thread) => {
            let mut existing_content = existing_thread.content;
            existing_content.append(&mut content);
            debug!("Found existing thread, will append content.");
            (existing_content, true)
        },
        None => {
            debug!("No existing thread found, will create a new one.");
            (content, false)
        }
    };

    // If the thread exists in the DB, we need to overwrite it.
    // If not, we need to create a new thread.

    // We also need to find the topic of the thread, which should be the user input (for now).
    let topic = content.iter().rev().find_map(|variant| {
        match variant {
            types::StreamVariant::User(input) => Some(input),
            _ => None,
        }
    });
    // There should always be a User input there because at least the examples contain one. 
    let topic = match topic {
        Some(topic) => {
            debug!("Found topic: {:?}", topic);
            topic.to_owned()
        },
        None => {
            debug!("No topic found, will use a placeholder.");
            "No topic found".to_owned()
        }
    };
        
    debug!("Found topic: {:?}", topic);

    let date = chrono::Utc::now().to_rfc3339(); // Also ISO 8601 compliant

    let content_bson = mongodb::bson::to_bson(&content);
    let content_bson = match content_bson {
        Ok(content_bson) => content_bson,
        Err(e) => {
            warn!("Failed to convert content to BSON: {:?}; cannot store thread!", e);
            return;
        }
    };

    // If the topic exists, we need to update the thread.
    if thread_exists {
        let result = database.clone().collection::<MongoDBThread>(&MONGODB_COLLECTION_NAME).update_one(
            doc! {
                "thread_id": thread_id
            },
            doc! {
                "$set": {
                    "content": content_bson,
                    "date": date,
                    "topic": topic,
                    "user_id": user_id,
                }
            },
        ).await;
    
        match result{
            Ok(update_result) => {
                debug!("Updated thread in database.");
                trace!("Update result: {:?}", update_result);
            },
            Err(e) => {
                warn!("Failed to update thread in database: {:?}; cannot store thread!", e);
            }
        }
    } else {
        // The thread does not exist, so we need to create a new one.
        let thread = MongoDBThread {
            user_id: user_id.to_string(),
            thread_id: thread_id.to_string(),
            date,
            topic,
            content,
        };

        let result = database.collection::<MongoDBThread>(&MONGODB_COLLECTION_NAME).insert_one(
            thread,
        ).await;

        match result {
            Ok(insert_result) => {
                debug!("Inserted thread into database.");
                trace!("Insert result: {:?}", insert_result);
            },
            Err(e) => {
                warn!("Failed to insert thread into database: {:?}; cannot store thread!", e);
            }
        }
    }
}



/// Loads a thread from the mongoDB database, by thread_id.
/// Also loads all other data from the thread, such as the user_id, date and "topic".
pub async fn read_thread(thread_id: &str, database: Database) -> Option<MongoDBThread> {
    debug!("Will load thread with id {}", thread_id);

    // Query the database by thread_id.
    let result = database.collection(&MONGODB_COLLECTION_NAME).find_one(
        doc! {
            "thread_id": thread_id
        },
    ).await;

    match result {
        Ok (inner) => {
            debug!("Loaded thread from database.");
            // The thread may or may not exist, but we just return the option.
            inner
        },
        Err (e) => {
            info!("Failed to load thread: {:?}; expecting it to not exist", e);
            None
        }
    }

}

/// Recieves a user_id and returns the last 10 threads of the user.
pub async fn read_threads(user_id: &str, database: Database) -> Vec<MongoDBThread> {
    debug!("Will load threads for user {}", user_id);

    // Query the database by user_id.
    let result = database.collection::<MongoDBThread>(&MONGODB_COLLECTION_NAME).find(
        doc! {
            "user_id": user_id
        }
    ).limit(-10) // Don't do 10 requests, do a single one for all 10. 
    .sort(doc! {
        "date": -1
    }).await;

    match result {
        Ok (mut inner) => {
            debug!("Loaded threads from database.");
            // The logic for collecting the theads is a bit tricky.
            let mut thread_vec = Vec::new();
            // inner.collect::<Vec<MongoDBThread>>().await.unwrap_or_default().into()
            while let Ok(Some(inner)) = inner.try_next().await {
                thread_vec.push(inner);
            };
            // Is the order correct? TODO!
            thread_vec

        },
        Err (e) => {
            info!("Failed to load threads: {:?}; expecting it to not exist", e);
            vec![]
        }
    }
}

/// Constructs a MongoDB database connection using the Vault URL.
/// If no Vault URL is given, the manually constructed database connection (which is mainly for testing purposes) is used.
pub async fn get_database(vault_url: Option<&str>) -> Result<Database, HttpResponse> {
    let mongodb_uri = match vault_url {
        Some(vault_url) => get_mongodb_uri(vault_url).await?, // Bubble the error up if it fails.
        None => {
            warn!("Using local MongoDB connection!");
            warn!("Make sure the clients will be using the main authentication method in the future.");
            // The thread_id means that we are using a manually constructed database connection.
            // Format: mongodb://<username>:<password>@<host>:<port>


            // Depending on whether it's the testing or production environment, we have different hosts. 
            // Testing uses localhost, but production uses the containers' host.

            #[cfg(target_os="macos")]
            let host = "localhost"; // For testing on macOS, we use localhost.
            #[cfg(not(target_os="macos"))]
            let host = "host.docker.internal"; // Docker for Linux uses this to access the host machine.

            let mongodb_uri = format!(
                "mongodb://{}:{}@{}:8503",
                MONGODB_USERNAME.as_str(),
                MONGODB_PASSWORD.as_str(),
                host,
            );  
            debug!("Using local MongoDB connection: {}", mongodb_uri);
            mongodb_uri
        }
    };

    // We have a URI to connect to, so we can create a MongoDB client.
    let client = match mongodb::Client::with_uri_str(&mongodb_uri).await {
        Ok(client) => {
            debug!("Successfully connected to MongoDB at {}", mongodb_uri);
            client
        },
        Err(e) => {
            error!("Failed to connect to MongoDB: {:?}", e);
            return Err(HttpResponse::InternalServerError().body("Failed to connect to MongoDB"));
        }
    };
        
    // Basic test: is mongoDB up? List the databases.
    let databases = client.list_database_names().await;
    match databases {
        Ok(dbs) => {
            debug!("MongoDB is up and running. Databases: {:?}", dbs);
        },
        Err(e) => {
            // We treat this as a warning, because it might be that the MongoDB server is not running.
            error!("Failed to make sure the MongoDB is running: {:?}", e);
            return Err(HttpResponse::InternalServerError().body("MongoDB is not running or cannot be reached"));
        }
    }

    //TODO: Maybe initialize the database? MongoDB is a bit finnicky about that.
    // While that wasn't tested explicetly, the testing battery ran 15/15 even with a fresh database.

    // We don't need the entire client, just the database.
    let database = client.database(&MONGODB_DATABASE_NAME);
    debug!("Using database: {}", *MONGODB_DATABASE_NAME);
    Ok(database)

}

static MONGODB_DATABASE_NAME: Lazy<String> = Lazy::new(|| {
    env::var("MONGODB_DATABASE_NAME").expect("\nMONGODB_DATABASE_NAME is not set in the .env file.\n")
});

static MONGODB_COLLECTION_NAME: Lazy<String> = Lazy::new(|| {
    env::var("MONGODB_COLLECTION_NAME").expect("\nMONGODB_COLLECTION_NAME is not set in the .env file.\n")
});

static MONGODB_USERNAME: Lazy<String> = Lazy::new(|| {
    env::var("LOCAL_MONGODB_USER").expect("\nLOCAL_MONGODB_USER is not set in the .env file.\n")
});

static MONGODB_PASSWORD: Lazy<String> = Lazy::new(|| {
    env::var("LOCAL_MONGODB_PASSWORD").expect("\nLOCAL_MONGODB_PASSWORD is not set in the .env file.\n")
});