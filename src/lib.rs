use custom_logger::*;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

// schema for the declarative_config
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeclarativeConfig {
    pub schema: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "defaultChannel")]
    pub default_channel: Option<String>,
    pub description: Option<String>,
    pub package: Option<String>,
    pub entries: Option<Vec<ChannelEntry>>,
    pub properties: Option<Vec<Property>>,
    pub image: Option<String>,
    #[serde(rename = "relatedImages")]
    pub related_images: Option<Vec<RelatedImage>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChannelEntry {
    pub name: String,
    pub replaces: Option<String>,
    pub skips: Option<Vec<String>>,
    #[serde(rename = "skipRange")]
    pub skip_range: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RelatedImage {
    pub name: String,
    pub image: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub schema: String,
    pub package: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    #[serde(rename = "type")]
    pub type_prop: String,
    pub value: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Value {
    #[serde(rename = "packageName")]
    pub package_name: Option<String>,
}

impl DeclarativeConfig {
    pub fn get_packages(dir: &String) -> Result<Vec<String>, Box<dyn Error>> {
        let mut packages = vec![];
        let paths = fs::read_dir(dir)?;
        for p in paths.into_iter() {
            packages.push(p.unwrap().file_name().into_string().unwrap());
        }
        Ok(packages)
    }

    pub fn read_operator_catalog<P>(in_file: P) -> Result<DeclarativeConfig, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        // Open the path in read-only mode, returns `io::Result<File>`
        let mut file = File::open(&in_file)?;

        // Read the file contents into a string, returns `io::Result<usize>`
        let mut s = String::new();
        file.read_to_string(&mut s)?;

        Ok(
            // check if we have yaml or json in the raw data
            if s.contains('{') {
                serde_json::from_str::<Self>(&s).unwrap()
            } else {
                serde_yaml::from_str::<Self>(&s).unwrap()
            },
        )
    }

    pub fn build_updated_configs<P>(base_dir: P) -> Result<(), Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        for entry in WalkDir::new(base_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
        {
            let file_name = entry.path();
            let file_name_str = file_name.to_string_lossy();

            // Open the path in read-only mode, returns `Result()`
            let mut f = File::open(file_name)?;

            let component = file_name_str.split("/configs/").nth(1).unwrap();
            trace!("updating config : {:#?}", component);

            // Read the file contents into a string, returns `io::Result<usize>`
            let mut s = String::new();
            f.read_to_string(&mut s)?;

            // check if we have yaml or json in the raw data
            if s.contains('{') {
                // break the declarative config into chunks
                // similar to what ibm have done in the breakdown of catalogs
                if file_name_str.contains("catalog.json") || file_name_str.contains("index.json") {
                    let mut chunks = s.split("}\n{");
                    let count = chunks.clone().count();
                    if count <= 1 {
                        chunks = s.split("}{")
                    }
                    let l = chunks.clone().count();
                    let mut update = "".to_string();
                    for (pos, item) in chunks.enumerate() {
                        // needs some refactoring
                        // first chunk
                        if pos == 0 {
                            update = item.to_string() + "}";
                        }
                        // last chunk
                        if pos == l - 1 {
                            update = "{".to_string() + item;
                        }
                        // everything in between
                        if pos > 0 && pos <= l - 2 {
                            update = "{".to_string() + item + "}";
                        }

                        // shadow update with a replace "null" - absolute crap usage of json,
                        // not sure why anyone would throw in a null or random value
                        let re = Regex::new(
                                r"(\x22value\x22: [0-9\.]+)|(\x22value\x22: \x22[0-9\.]+\x22)|(\x22value\x22: null)",
                            ).unwrap();
                        let new_update = re.replace_all(&update, "\"value\": {\"group\":\"\"}");
                        let mut dir = file_name_str.split("catalog.json").nth(0).unwrap();
                        if dir.contains("index.json") {
                            dir = file_name_str.split("index.json").nth(0).unwrap();
                        }

                        // parse the file (we know its json)
                        let dc = serde_json::from_str::<Self>(&new_update);
                        match dc {
                            Ok(dc) => {
                                let name = dc.name.clone();
                                if name.is_some() {
                                    // now marshal to json (this cleans all unwanted fields)
                                    // and finally write to disk

                                    let json_contents = serde_json::to_string(&dc).unwrap();
                                    let update_dir = Path::new(dir).join("updated-configs");
                                    fs::create_dir_all(&update_dir).expect("must create dir");
                                    fs::write(
                                        update_dir.join(name.unwrap() + ".json"),
                                        json_contents.as_str(),
                                    )
                                    .expect("must write updated json file");
                                } else {
                                    warn!(
                                        "could not decode decalarative config for {}",
                                        &component
                                    );
                                }
                            }
                            Err(err) => {
                                warn!("could not parse : {:#?} : {} : {}", &component, pos, err);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_declarativeconfig_map<P>(base_dir: P) -> HashMap<String, Self>
    where
        P: AsRef<Path>,
    {
        let mut dc_list = HashMap::new();

        for entry in WalkDir::new(base_dir.as_ref())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
        {
            let res = DeclarativeConfig::read_operator_catalog(entry.path());
            if res.is_ok() {
                let kv = res.unwrap();
                let key = format!(
                    "{}={}",
                    kv.name.as_ref().unwrap(),
                    kv.schema.as_ref().unwrap()
                );
                dc_list.insert(
                    key,
                    DeclarativeConfig::read_operator_catalog(entry.path()).unwrap(),
                );
            } else {
                warn!(
                    "could not locate declarative config {}",
                    entry.path().to_string_lossy()
                );
            }
        }
        dc_list
    }
}

#[cfg(test)]
mod tests {
    // this brings everything from parent's scope into this scope
    use super::*;

    #[test]

    fn build_update_configs_pass() {
        Logging::new().init().expect("should initialize");
        let res = DeclarativeConfig::build_updated_configs("tests");
        info!("{:#?}", res);
    }
}
