use polydeck_core::chat_history::HistoryStore;

#[tokio::test]
async fn test_chat_history_db() {
    let store = HistoryStore::open_in_memory().unwrap();
    let count = store.sync_all().unwrap();
    println!("Synced count into in-memory DB: {}", count);
    let list = store.list_summaries().unwrap();
    println!("List summaries count: {}", list.len());
    for s in list.iter().take(5) {
        println!(
            " - [{}] {} ({}) - {} msgs, {} tokens",
            s.client, s.title, s.id, s.message_count, s.total_tokens
        );
    }
    let stats = store.get_usage_stats().unwrap();
    println!(
        "Stats: total_sessions = {}, total_messages = {}, total_tokens = {}",
        stats.total_sessions, stats.total_messages, stats.total_tokens
    );
    assert!(list.len() > 0, "Should have indexed sessions in memory");
}
