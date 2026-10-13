//! Zhi-Mart backend: admin authentication API (password + email OTP, JWT access
//! tokens, rotating refresh tokens with a hard 30-day session lifetime).

pub mod app;
pub mod auth;
pub mod cli;
pub mod config;
pub mod email;
pub mod error;
pub mod jobs;
pub mod middleware;
pub mod money;
pub mod net;
pub mod payments;
pub mod repo;
pub mod routes;
pub mod state;
pub mod telemetry;
