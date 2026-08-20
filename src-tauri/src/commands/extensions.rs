use tauri::command;

#[command]
pub async fn ad_list_mcp_servers() -> Result<Vec<polydeck_core::mcp::McpServer>, String> {
    Ok(polydeck_core::mcp::builtin_servers())
}

#[command]
pub async fn ad_list_skills() -> Result<Vec<polydeck_core::skills::ManagedSkill>, String> {
    Ok(vec![])
}

#[command]
pub async fn ad_list_prompts() -> Result<Vec<polydeck_core::prompts::PromptTemplate>, String> {
    Ok(vec![])
}
