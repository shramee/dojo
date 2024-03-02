use katana_rpc_api::ApiKind;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub max_connections: u32,
    pub max_request_body_size: u32,
    pub apis: Vec<ApiKind>,
}

impl ServerConfig {
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
