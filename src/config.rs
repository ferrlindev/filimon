use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    ls: Vec<String>,
}

pub fn load_config(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&raw)?;
    Ok(config.ls)
}
