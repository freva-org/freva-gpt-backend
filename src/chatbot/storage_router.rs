use mongodb::Database;

use crate::chatbot::mongodb::mongodb_storage;

use super::types::Conversation;

#[allow(dead_code)] // Only one variant of this enum is ever used, so this shuts up the warning
/// Represents the possible available storage options for the threads
pub enum AvailableStorages {
    Disk,
    MongoDB,
}

/// The currently active storage for the threads
pub static STORAGE: AvailableStorages = AvailableStorages::MongoDB;

/// Appends a thread to the storage. User_Id is ignored for the disk storage.
pub async fn append_thread(
    thread_id: &str,
    user_id: &str,
    content: Conversation,
    database: Database,
) {
    match STORAGE {
        AvailableStorages::Disk => {
            super::thread_storage::append_thread(thread_id, content);
        }
        AvailableStorages::MongoDB => {
            mongodb_storage::append_thread(thread_id, user_id, content, database).await;
        }
    }
}

/// Reads a thread from the storage. Returns an error if the thread is not found, most likely because it doesn't exist.
pub async fn read_thread(
    thread_id: &str,
    database: Database,
) -> Result<Conversation, std::io::Error> {
    match STORAGE {
        AvailableStorages::Disk => super::thread_storage::read_thread(thread_id),
        AvailableStorages::MongoDB => {
            match mongodb_storage::read_thread(thread_id, database).await {
                Some(thread) => Ok(thread.content),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Thread not found",
                )),
            }
        }
    }
}
