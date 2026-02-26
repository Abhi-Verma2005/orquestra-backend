pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub openai_api_key: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            database_url: require_env("DATABASE_URL"),
            jwt_secret: require_env("JWT_SECRET"),
            openai_api_key: require_env("OPENAI_API_KEY"),
            port: std::env::var("SERVER_PORT")
                .unwrap_or("8080".to_string())
                .parse()
                .expect("SERVER_PORT must be a number"),
        }
    }
}

fn require_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Missing required env var: {key}"))
}
