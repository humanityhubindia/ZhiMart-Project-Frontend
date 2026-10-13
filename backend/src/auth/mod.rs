//! Admin authentication primitives: password hashing, access tokens, OTP codes,
//! refresh tokens, the refresh/device cookies, CSRF checks, login throttle keys
//! and the auth extractor.

pub mod cookies;
pub mod csrf;
pub mod extractor;
pub mod jwt;
pub mod otp;
pub mod password;
pub mod throttle;
pub mod tokens;

pub use csrf::CsrfGuard;
pub use extractor::AuthAdmin;
