# Orquestra - AI Orchestrator Project Context

## Project Overview
**Project Name:** orquestra  
**Type:** Modular, high-performance AI orchestrator with sidebar/chat interface  
**Inspiration:** Cursor AI  
**Backend:** Rust with async runtime

## Technical Stack
- **Runtime:** `tokio` for async operations
- **Web Framework:** `axum` for WebSockets and HTTP
- **Serialization:** `serde` for JSON handling
- **Database:** `sqlx` for persistence
- **Caching:** `redis` for ephemeral state
- **Architecture:** Fully modular, high-performance concurrent backend

## Core Modules Architecture

### 1. **orchestrator/** 
- Manages planning, execution, and tool invocation
- Core orchestration logic and workflow management

### 2. **tools/**
- Each tool implements a common `Tool` trait (name + run)
- Examples: WebSearch, FileEdit, CodeSearch
- Modular and extensible tool registry

### 3. **llm_provider/**
- Abstract interface for LLM providers
- Supports: Cursor AI, OpenAI, Anthropic
- Decoupled from orchestrator for flexibility

### 4. **context/**
- Short-term memory management
- Long-term vector store for embeddings
- User metadata and preferences storage

### 5. **websocket/**
- Message routing and serialization
- Event streaming to frontend
- Real-time communication layer

### 6. **persistence/**
- Postgres for structured data storage
- Redis for caching and ephemeral state
- Database layer abstraction

### 7. **permissions/**
- Safety and guardrails implementation
- User confirmations for destructive operations
- Security and access control

## Workflow Process
1. **Receive** user message via WebSocket
2. **Plan** - Planner generates JSON plan of todos (tool calls) based on user request + context
3. **Execute** - Executor runs tool calls sequentially or concurrently, streams events back
4. **Update** - Context and metadata are updated (user preferences, tool usage, memory)
5. **Stream** - Events are streamed back to frontend in real-time

## Phased Development Plan

### Phase 1: Foundation
- Minimal orchestration loop
- Dummy tools implementation
- Basic WebSocket communication

### Phase 2: LLM Integration
- Integrate Cursor AI as planner
- Real plan generation capabilities
- Enhanced tool orchestration

### Phase 3: Tool Expansion
- Expand tool registry
- Tool chaining capabilities
- Retry and timeout logic

### Phase 4: Context Enhancement
- Implement context/memory features
- Vector store integration
- Advanced memory management

### Phase 5: Advanced Features
- Dynamic tools
- Middleware system
- Full frontend integration
- Metrics and monitoring

## Project Goals
- **Modularity:** Fully modular, reusable orchestrator core for multiple client projects
- **Performance:** High-performance concurrent Rust backend capable of handling hundreds of sessions
- **Separation of Concerns:** Clear boundaries between orchestrator, tools, LLM provider, context, and frontend
- **Seamless Flow:** Plan → Execute → Update → Stream workflow integration

## Development Expectations
- Maintain modular architecture throughout development
- Follow Rust best practices for async/concurrent programming
- Keep clear separation between modules
- Ensure high performance and scalability
- Align all code suggestions with this architectural vision

## Current Status
- ✅ Project structure initialized
- ✅ All core modules created with placeholder files
- ✅ Cargo.toml configured with necessary dependencies
- 🔄 Ready for Phase 1 implementation

---
*This document serves as the single source of truth for project context and should be referenced for all development decisions.*
