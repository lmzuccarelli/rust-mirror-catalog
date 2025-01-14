use custom_logger as log;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Read;
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
            packages.push(p.unwrap().file_name().to_os_string().into_string().unwrap());
        }
        Ok(packages)
    }

    pub fn read_operator_catalog(in_file: String) -> Result<DeclarativeConfig, Box<dyn Error>> {
        // Open the path in read-only mode, returns `io::Result<File>`
        let mut file = match File::open(&in_file) {
            Err(why) => panic!("couldn't open {}: {}", in_file, why),
            Ok(file) => file,
        };

        // Read the file contents into a string, returns `io::Result<usize>`
        let mut s = String::new();
        file.read_to_string(&mut s)?;
        let dc: DeclarativeConfig;

        // check if we have yaml or json in the raw data
        if s.contains("{") {
            dc = serde_json::from_str::<Self>(&s).unwrap();
        } else {
            dc = serde_yaml::from_str::<Self>(&s).unwrap();
        }
        Ok(dc)
    }

    pub fn build_updated_configs(base_dir: String) -> Result<(), Box<dyn Error>> {
        for entry in WalkDir::new(base_dir.clone())
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.path().is_file() {
                let file_name = "".to_string() + entry.path().display().to_string().as_str();

                // Open the path in read-only mode, returns `Result()`
                let mut f = match File::open(&file_name) {
                    Err(why) => panic!("couldn't open {}: {}", file_name, why),
                    Ok(file) => file,
                };

                let component = &file_name.split("/configs/").nth(1).unwrap();
                log::trace!("updating config : {:#?}", &component);

                // Read the file contents into a string, returns `io::Result<usize>`
                let mut s = String::new();
                f.read_to_string(&mut s)?;

                // check if we have yaml or json in the raw data
                if s.contains("{") {
                    // break the declarative config into chunks
                    // similar to what ibm have done in the breakdown of catalogs
                    if file_name.contains("catalog.json") {
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

                            let dir = file_name.split("catalog.json").nth(0).unwrap();
                            // parse the file (we know its json)
                            let dc = serde_json::from_str::<Self>(&new_update.clone());
                            match dc {
                                Ok(dc) => {
                                    let name = dc.clone().name.unwrap().to_string();
                                    // now marshal to json (this cleans all unwanted fields)
                                    // and finally write to disk
                                    let json_contents = serde_json::to_string(&dc).unwrap();
                                    let update_dir =
                                        dir.to_string() + "/updated-configs/" + &name + ".json";

                                    fs::create_dir_all(dir.to_string() + "/updated-configs")
                                        .expect("must create dir");
                                    fs::write(update_dir.clone(), json_contents.clone())
                                        .expect("must write updated json file");
                                }
                                Err(err) => {
                                    log::error!(
                                        "could not parse : {:#?} : {} : {}",
                                        &component,
                                        pos,
                                        err.to_string()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_declarativeconfig_map(base_dir: String) -> HashMap<String, Self> {
        let mut dc_list = HashMap::new();

        for entry in WalkDir::new(base_dir.clone())
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.path().is_file() {
                let file_name =
                    base_dir.clone() + entry.path().file_name().unwrap().to_str().unwrap();
                let res = DeclarativeConfig::read_operator_catalog(file_name.clone()).unwrap();
                let name = res.clone().name.clone();
                let schema = res.clone().schema.clone();
                let key = name.clone().unwrap() + "=" + schema.clone().unwrap().as_str();
                dc_list.insert(
                    key,
                    DeclarativeConfig::read_operator_catalog(file_name).unwrap(),
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
        log::Logging::new().init().expect("should initialize");
        let res = DeclarativeConfig::build_updated_configs("tests".to_string());
        log::info!("{:#?}", res);
    }
}
