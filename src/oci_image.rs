use crate::images::ImageRepository;
use failure::Error;
use serde::Deserialize;
use serde_json::from_str;
use std::collections::HashMap;
use std::fs::{read_to_string, File};
use std::path::PathBuf;
use std::str::FromStr;
use tar::Archive;
use tempdir::TempDir;

const OCI_IMAGE_TEMP: &'static str = "ruthless_oci_image";
const OCI_IMAGE_REPOSITORIES_PATH: &'static str = "repositories";

#[derive(Deserialize)]
struct LayerJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
}

#[derive(Deserialize, Clone)]
struct OCIImageRepositoriesFileLatest {
    latest: String,
}
type OCIImageRepositoriesFile = HashMap<String, OCIImageRepositoriesFileLatest>;

pub(crate) struct OCIImage {
    tar_content: TempDir,
    name: String,
}

#[derive(Debug, Fail)]
enum OCIImageError {
    #[fail(display = "No layer name")]
    LayerHasNoName,
    #[fail(display = "The image has no layers")]
    NoLayers,
    #[fail(display = "Tar path has no file name.")]
    NoFileName,
    #[fail(display = "Repositories file doesn't have key {} between {:?}", 0, 1)]
    RepositoryFileIncomplete(String, Vec<String>),
}

#[inline]
fn layer_name<'a>(
    container_name: &'a str,
    layer: &'a PathBuf,
    pending_layers: &'a Vec<String>,
) -> Result<&'a str, Error> {
    if pending_layers.len() == 0 {
        Ok(container_name)
    } else {
        Ok(layer
            .file_name()
            .ok_or(OCIImageError::LayerHasNoName)?
            .to_str()
            .ok_or(OCIImageError::NoFileName)?)
    }
}

impl OCIImage {
    pub(crate) fn new(tar_file_path: &str) -> Result<OCIImage, Error> {
        let path = PathBuf::from_str(tar_file_path)?;
        let name = path
            .file_name()
            .ok_or(OCIImageError::NoFileName)?
            .to_str()
            .ok_or(OCIImageError::NoFileName)?
            .to_owned()
            .split(".")
            .collect::<Vec<&str>>()[0]
            .to_owned();
        let mut tar_file = Archive::new(File::open(tar_file_path)?);
        let tar_content = TempDir::new(OCI_IMAGE_TEMP)?;
        tar_file.unpack(&tar_content.path())?;
        Ok(OCIImage { tar_content, name })
    }

    pub(crate) fn import(&self, image_repository: &ImageRepository) -> Result<(), Error> {
        let repositories_content = self.extract_repositories_content()?;
        let latest_content = self.extract_latest_content(&repositories_content)?;
        let mut layer_stack = self.build_layer_stack(latest_content.latest.as_str())?;
        self.import_from_layer_stack(image_repository, &mut layer_stack)?;
        Ok(())
    }

    #[inline]
    fn import_from_layer_stack(
        &self,
        image_repository: &ImageRepository,
        layer_stack: &mut Vec<String>,
    ) -> Result<(), Error> {
        let first_layer = self
            .tar_content
            .path()
            .join(layer_stack.pop().ok_or(OCIImageError::NoLayers)?);
        let mut last_layer_processed =
            layer_name(self.name.as_str(), &first_layer, &layer_stack)?.to_owned();
        image_repository.create_image_from_path(
            last_layer_processed.as_str(),
            &first_layer.join("layer.tar"),
        )?;
        while let Some(layer) = layer_stack.pop().map(|v| v.clone()) {
            let layer_path = self.tar_content.path().join(layer);
            let new_layer_name = layer_name(self.name.as_str(), &layer_path, layer_stack)?;
            image_repository.create_layer_for_image(
                new_layer_name,
                last_layer_processed.as_str(),
                &layer_path.join("layer.tar"),
            )?;
            last_layer_processed = new_layer_name.to_owned();
        }
        Ok(())
    }

    #[inline]
    fn build_layer_stack(&self, starting_layer: &str) -> Result<Vec<String>, Error> {
        let mut results = vec![starting_layer.to_owned()];
        let file_path = self.tar_content.path().join(starting_layer);
        let mut layer_json = from_str::<LayerJson>(read_to_string(&file_path)?.as_str())?;
        while let Some(parent) = layer_json.parent.clone() {
            let next_layer_path = self.tar_content.path().join(parent.as_str());
            results.push(parent);
            layer_json = from_str::<LayerJson>(read_to_string(&next_layer_path)?.as_str())?;
        }
        Ok(results)
    }

    #[inline]
    fn extract_latest_content<'a>(
        &self,
        repositories_content: &'a OCIImageRepositoriesFile,
    ) -> Result<&'a OCIImageRepositoriesFileLatest, Error> {
        Ok(repositories_content
            .get(&self.name)
            .ok_or(OCIImageError::RepositoryFileIncomplete(
                self.name.clone(),
                repositories_content.keys().map(|s| s.clone()).collect(),
            ))?)
    }

    #[inline]
    fn extract_repositories_content(&self) -> Result<OCIImageRepositoriesFile, Error> {
        Ok(from_str::<OCIImageRepositoriesFile>(
            read_to_string(self.tar_content.path().join(OCI_IMAGE_REPOSITORIES_PATH))?.as_str(),
        )?)
    }
}
