# Halo Backend

Rust/Axum backend for the Halo AI chat application.

## Requirements
- Rust 1.75+
- PostgreSQL 14+
- OpenAI API key

## Setup

1. Copy `.env.example` to `.env` and fill in values
2. Create database: `createdb halo`
3. Run: `cargo run`

Migrations run automatically on startup.

## Env vars
| Variable | Description |
|---|---|
| DATABASE_URL | PostgreSQL connection string |
| JWT_SECRET | Secret for signing JWTs (min 32 chars) |
| OPENAI_API_KEY | OpenAI API key |
| SERVER_PORT | Port to listen on (default: 8080) |

## API

See `frontend-readme.md` in the tasks/ folder for full API documentation.
