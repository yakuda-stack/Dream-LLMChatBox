use serde::Deserialize;

#[derive(Deserialize)]
pub struct CatalogModel {
    pub name: String,
    pub group: String,
    pub categories: Vec<String>,
    pub description: String,
    pub capabilities: Vec<String>,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub variants: Vec<Variant>,
}

#[derive(Deserialize)]
pub struct Variant {
    pub tag: String,
    pub size_gb: f64,
}

impl Variant {
    pub fn size_bytes(&self) -> u64 {
        (self.size_gb * 1e9) as u64
    }

    pub fn label(&self) -> &str {
        self.tag.split_once(':').map(|(_, label)| label).unwrap_or(&self.tag)
    }
}

pub fn load() -> Vec<CatalogModel> {
    serde_json::from_str(include_str!("../assets/models.json")).unwrap_or_default()
}

pub fn find<'a>(catalog: &'a [CatalogModel], installed_name: &str) -> Option<&'a CatalogModel> {
    let family = installed_name.split(':').next().unwrap_or(installed_name);
    catalog.iter().find(|model| model.name == family)
}
