//! Razorpay online payments: REST client, signature verification, the order
//! access token that lets a guest (no account) pay for / view their own order,
//! and the payment service used by the HTTP handlers and the webhook.

pub mod order_token;
pub mod razorpay;
pub mod service;
pub mod signature;

pub use razorpay::RazorpayClient;
