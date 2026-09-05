use polydeck_core::chat_history::HistoryStore;

/// Exercise the history store against whatever transcripts the host has.
///
/// `sync_all` reads the real Claude and Codex session directories, so how much
/// it finds is a property of the machine, not of the code. Asserting a non-empty
/// result made this fail on any host without local transcripts — every CI runner
/// included. So the query paths are checked unconditionally and the assertions
/// that need transcripts run only when some were found.
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

    // The list used to be capped at 500 rows while the stats counted the whole
    // table, and this assertion encoded that cap as intended behaviour. It was not:
    // on a database holding 1010 rows the cap hid 623 of them, which is what "my
    // history disappeared after switching providers" turned out to be. The two must
    // now agree exactly, at any size.
    assert_eq!(
        stats.total_sessions,
        list.len(),
        "usage stats report {} sessions but the summary list holds {}; the list must not truncate",
        stats.total_sessions,
        list.len()
    );

    if count == 0 {
        println!("no local transcripts on this host; skipping the indexed-content assertions");
        return;
    }
    assert!(
        !list.is_empty(),
        "sync_all reported {count} sessions but the summary list is empty"
    );
    assert!(
        list.iter().all(|s| !s.id.is_empty()),
        "every indexed session needs an id"
    );
}
