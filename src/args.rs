use clap::Parser;
use serde::Deserialize;
use std::error::Error;
use std::fs;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long, value_delimiter = ',', help = "Comma-separated list to check")]
    pub ls: Option<Vec<String>>,
    #[arg(long, default_value = "config.json", help = "Path to the config file")]
    pub config: String,
}

impl Args {
    fn produce(&mut self) -> &Vec<String> {
        if self.ls.is_none() {
            self.ls = Some(load_config(&self.config).unwrap());
        }
        self.ls.as_ref().unwrap()
    }

    pub fn produce_links(&mut self) -> Vec<&str> {
        self.produce().iter().map(|s| s.as_str()).collect()
    }
}

#[derive(Deserialize)]
struct Config {
    pub ls: Vec<String>,
}

fn load_config(config_path: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let content = fs::read_to_string(config_path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config.ls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_produce_links_with_lns_provided() {
        let args = Args {
            ls: Some(vec!["link1".to_string(), "link2".to_string()]),
            config: "dummy_config.json".to_string(),
        };
        let links = args.produce_links();
        assert_eq!(links, vec!["link1".to_string(), "link2".to_string()]);
    }

    #[test]
    fn test_produce_links_without_lns_loads_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_config.json");
        let file_path_str = file_path.to_str().unwrap();

        let json_content = r#"{"lns": ["config_link1", "config_link2"]}"#;
        let mut file = fs::File::create(file_path_str).unwrap();
        file.write_all(json_content.as_bytes()).unwrap();

        let args = Args {
            ls: None,
            config: file_path_str.to_string(),
        };
        let links = args.produce_links();
        assert_eq!(
            links,
            vec!["config_link1".to_string(), "config_link2".to_string()]
        );

        // TempDir cleans up automatically on drop
    }

    #[test]
    #[should_panic(expected = "called `Result::unwrap()` on an `Err` value")]
    fn test_produce_links_panics_on_load_config_error() {
        let args = Args {
            ls: None,
            config: "nonexistent_config.json".to_string(),
        };
        let _ = args.produce_links(); // Should panic on unwrap() due to file not found
    }

    #[test]
    fn test_load_config_valid_json() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("valid_config.json");
        let file_path_str = file_path.to_str().unwrap();

        let json_content = r#"{"lns": ["valid_link1", "valid_link2"]}"#;
        let mut file = fs::File::create(file_path_str).unwrap();
        file.write_all(json_content.as_bytes()).unwrap();

        let loaded_links = load_config(file_path_str).unwrap();
        assert_eq!(
            loaded_links,
            vec!["valid_link1".to_string(), "valid_link2".to_string()]
        );

        // TempDir cleans up automatically
    }

    #[test]
    #[should_panic(expected = "called `Result::unwrap()` on an `Err` value")]
    fn test_load_config_invalid_json_panics_on_unwrap() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("invalid_config.json");
        let file_path_str = file_path.to_str().unwrap();

        let invalid_json = r#"{"lns": ["link1", ]}"#; // Invalid trailing comma
        let mut file = fs::File::create(file_path_str).unwrap();
        file.write_all(invalid_json.as_bytes()).unwrap();

        let _ = load_config(file_path_str).unwrap(); // Panics on deserialization error

        // TempDir cleans up automatically (won't reach here)
    }
}
