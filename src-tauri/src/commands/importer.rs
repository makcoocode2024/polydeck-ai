use tauri::command;

#[command]
pub async fn ad_detect_importable() -> Result<Vec<String>, String> {
    Ok(polydeck_core::importer::detect_importable_sources())
}

#[command]
pub async fn ad_import_from_provider_deck(_path: String) -> Result<(), String> {
    let result = polydeck_core::importer::import_from_provider_deck(
        polydeck_core::importer::ImportConflict::Skip,
    ).map_err(|e| e.to_string())?;
    if result.success {
        Ok(())
    } else {
        Err(result.message)
    }
}