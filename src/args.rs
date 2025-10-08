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
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_parse_default() {
        let args = Args::parse_from(vec!["prog"]);
        assert_eq!(args.ls, None);
        assert_eq!(args.config, "config.json");
    }

    #[test]
    fn test_parse_ls() {
        let args = Args::parse_from(vec!["prog", "--ls", "a,b,c"]);
        assert_eq!(
            args.ls,
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
        assert_eq!(args.config, "config.json");
    }

    #[test]
    fn test_parse_config() {
        let args = Args::parse_from(vec!["prog", "--config", "other.json"]);
        assert_eq!(args.ls, None);
        assert_eq!(args.config, "other.json");
    }

    #[test]
    fn test_produce_links_with_ls() {
        let mut args = Args {
            ls: Some(vec!["a".to_string(), "b".to_string()]),
            config: "config.json".to_string(),
        };
        let links = args.produce_links();
        assert_eq!(links, vec!["a", "b"]);
        assert_eq!(args.ls, Some(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn test_load_config_success() {
        let config_path = "test_config_success.json";
        let mut file = File::create(config_path).unwrap();
        file.write_all(b"{\"ls\": [\"x\", \"y\"]}").unwrap();
        let res = load_config(config_path).unwrap();
        assert_eq!(res, vec!["x".to_string(), "y".to_string()]);
        fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn test_load_config_file_not_found() {
        let res = load_config("nonexistent.json");
        assert!(res.is_err());
    }

    #[test]
    fn test_load_config_invalid_json() {
        let config_path = "test_config_invalid.json";
        fs::write(config_path, "not json").unwrap();
        let res = load_config(config_path);
        assert!(res.is_err());
        fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn test_produce_with_load_success() {
        let config_path = "test_config_produce.json";
        fs::write(config_path, "{\"ls\": [\"p\", \"q\"]}").unwrap();
        let mut args = Args {
            ls: None,
            config: config_path.to_string(),
        };
        let ls = args.produce();
        assert_eq!(ls, &vec!["p".to_string(), "q".to_string()]);

        let links = args.produce_links();
        assert_eq!(links, vec!["p", "q"]);
        assert_eq!(args.ls, Some(vec!["p".to_string(), "q".to_string()]));
        fs::remove_file(config_path).unwrap();
    }

    #[test]
    #[should_panic]
    fn test_produce_panic_file_not_found() {
        let mut args = Args {
            ls: None,
            config: "nonexistent.json".to_string(),
        };
        args.produce();
    }

    #[test]
    #[should_panic]
    fn test_produce_panic_invalid_json() {
        let config_path = "test_config_panic.json";
        fs::write(config_path, "not json").unwrap();
        let mut args = Args {
            ls: None,
            config: config_path.to_string(),
        };
        args.produce();
        fs::remove_file(config_path).unwrap();
    }
}
