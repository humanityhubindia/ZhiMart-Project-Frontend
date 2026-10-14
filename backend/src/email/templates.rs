//! Email rendering: the admin sign-in code and the admin "new order"
//! notification. Table layout + inline CSS so it renders consistently in
//! Gmail, Outlook (Word engine) and Apple Mail. No images, no remote content,
//! no tracking. Every interpolated value is HTML-escaped: order data comes
//! from anonymous customers and is untrusted.

use std::{fmt::Write as _, net::IpAddr};

use chrono::{DateTime, FixedOffset, Utc};
use secrecy::ExposeSecret;
use zeroize::Zeroizing;

use super::{OtpEmail, OutboundEmail};
use crate::{config::Config, payments::order_token, repo::orders::Order};

const BRAND: &str = "Zhi-Mart Admin";

/// A rendered message. Bodies contain the one-time code, so they are wiped
/// from memory when dropped.
pub struct RenderedEmail {
    pub subject: String,
    pub html: Zeroizing<String>,
    pub text: Zeroizing<String>,
}

/// Escape text for HTML element content and quoted attribute values.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '/' => out.push_str("&#x2F;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Plain-text bodies: strip control characters so a display name cannot
/// inject fake lines/headers into the text part.
fn sanitize_text(input: &str) -> String {
    input.chars().filter(|c| !c.is_control()).collect()
}

fn minutes_label(n: u64) -> String {
    if n == 1 {
        "1 minute".to_owned()
    } else {
        format!("{n} minutes")
    }
}

/// Render the sign-in code email.
pub fn render_otp_email(email: &OtpEmail, requested_at: DateTime<Utc>) -> RenderedEmail {
    let code = email.code.expose_secret();
    let name_raw = email.display_name.trim();
    let name_text = if name_raw.is_empty() {
        "there".to_owned()
    } else {
        sanitize_text(name_raw)
    };
    let validity = minutes_label(email.expires_in_minutes);
    let when = requested_at.format("%d %b %Y, %H:%M UTC").to_string();
    let ip: Option<IpAddr> = email.request_ip.filter(|ip| !ip.is_unspecified());

    let subject = format!("Your {BRAND} sign-in code");

    // ---- plain text ----
    let mut text = Zeroizing::new(String::with_capacity(768));
    let _ = writeln!(text, "{BRAND}\n");
    let _ = writeln!(text, "Hi {name_text},\n");
    let _ = writeln!(text, "Use this code to finish signing in:\n");
    let _ = writeln!(text, "    {code}\n");
    let _ = writeln!(
        text,
        "The code expires in {validity} and can only be used once.\n"
    );
    let _ = writeln!(text, "Requested: {when}");
    if let Some(ip) = ip {
        let _ = writeln!(text, "From IP address: {ip}");
    }
    let _ = writeln!(
        text,
        "\nIf you didn't try to sign in, your password may be compromised — reset it immediately.\n"
    );
    let _ = writeln!(
        text,
        "Never share this code. Zhi-Mart staff will never ask for it."
    );

    // ---- HTML ----
    let name_html = escape_html(&name_text);
    let code_html = Zeroizing::new(escape_html(code));
    let validity_html = escape_html(&validity);
    let when_html = escape_html(&when);
    let ip_row = match ip {
        Some(ip) => format!(
            r#"<tr><td style="padding:4px 0;color:#94a3b8;font-size:13px;">From IP address</td><td style="padding:4px 0;color:#e2e8f0;font-size:13px;text-align:right;font-family:Consolas,Menlo,monospace;">{}</td></tr>"#,
            escape_html(&ip.to_string())
        ),
        None => String::new(),
    };

    let mut html = Zeroizing::new(String::with_capacity(4096));
    let _ = write!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>{subject}</title>
</head>
<body style="margin:0;padding:0;background-color:#0f172a;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">Your sign-in code expires in {validity_html}.</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background-color:#0f172a;">
<tr><td align="center" style="padding:32px 16px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:480px;background-color:#1e293b;border:1px solid #334155;border-radius:12px;">
<tr><td style="padding:24px 32px;border-bottom:3px solid #f59e0b;">
<span style="font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:20px;font-weight:700;color:#f59e0b;letter-spacing:0.3px;">Zhi-Mart</span>
<span style="font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:20px;font-weight:400;color:#e2e8f0;"> Admin</span>
</td></tr>
<tr><td style="padding:28px 32px 8px 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;color:#e2e8f0;font-size:15px;line-height:22px;">
<p style="margin:0 0 12px 0;">Hi {name_html},</p>
<p style="margin:0 0 20px 0;">Use this code to finish signing in to the admin portal:</p>
</td></tr>
<tr><td align="center" style="padding:0 32px;">
<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="background-color:#0f172a;border:1px solid #f59e0b;border-radius:8px;">
<tr><td style="padding:16px 28px;font-family:Consolas,Menlo,'Courier New',monospace;font-size:34px;font-weight:700;letter-spacing:10px;color:#fbbf24;">{code}</td></tr>
</table>
</td></tr>
<tr><td style="padding:20px 32px 8px 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;color:#cbd5e1;font-size:14px;line-height:21px;">
<p style="margin:0;">This code expires in <strong style="color:#f8fafc;">{validity_html}</strong> and can only be used once.</p>
</td></tr>
<tr><td style="padding:12px 32px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<tr><td style="padding:4px 0;color:#94a3b8;font-size:13px;">Requested</td><td style="padding:4px 0;color:#e2e8f0;font-size:13px;text-align:right;">{when_html}</td></tr>
{ip_row}
</table>
</td></tr>
<tr><td style="padding:8px 32px 24px 32px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background-color:#451a03;border-left:4px solid #f59e0b;border-radius:6px;">
<tr><td style="padding:12px 16px;font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:13px;line-height:20px;color:#fde68a;">
<strong>Didn't try to sign in?</strong> If you didn't try to sign in, your password may be compromised — reset it immediately.
</td></tr>
</table>
</td></tr>
<tr><td style="padding:16px 32px;border-top:1px solid #334155;font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:12px;line-height:18px;color:#64748b;">
Never share this code. Zhi-Mart staff will never ask for it.<br>This is an automated security message from {brand}.
</td></tr>
</table>
</td></tr>
</table>
</body>
</html>
"#,
        subject = escape_html(&subject),
        code = code_html.as_str(),
        brand = escape_html(BRAND),
    );

    RenderedEmail {
        subject,
        html,
        text,
    }
}

// ---------------------------------------------------------------------------
// Admin "new order" notification
// ---------------------------------------------------------------------------

/// India Standard Time (UTC+05:30, no DST).
const IST_OFFSET_SECS: i32 = 5 * 3600 + 30 * 60;
const MAX_SUBJECT_NAME_CHARS: usize = 60;

/// Format paise as rupees with Indian digit grouping: `₹1,23,456` or
/// `₹1,650.50` (paise shown only when non-zero).
pub fn format_inr(paise: i64) -> String {
    let sign = if paise < 0 { "-" } else { "" };
    let abs = paise.unsigned_abs();
    let (rupees, frac) = (abs / 100, abs % 100);
    let digits = rupees.to_string();
    let grouped = if digits.len() <= 3 {
        digits
    } else {
        let (head, last3) = digits.split_at(digits.len() - 3);
        let mut out = String::with_capacity(digits.len() + digits.len() / 2);
        let first = head.len() % 2;
        if first == 1 {
            out.push_str(&head[..1]);
        }
        for (i, pair) in head.as_bytes()[first..].chunks(2).enumerate() {
            if i > 0 || first == 1 {
                out.push(',');
            }
            out.push_str(std::str::from_utf8(pair).unwrap_or_default());
        }
        out.push(',');
        out.push_str(last3);
        out
    };
    if frac == 0 {
        format!("{sign}₹{grouped}")
    } else {
        format!("{sign}₹{grouped}.{frac:02}")
    }
}

fn ist(ts: DateTime<Utc>) -> String {
    match FixedOffset::east_opt(IST_OFFSET_SECS) {
        Some(tz) => ts
            .with_timezone(&tz)
            .format("%d %b %Y, %I:%M %p IST")
            .to_string(),
        None => ts.format("%d %b %Y, %H:%M UTC").to_string(),
    }
}

fn payment_method_label(method: &str) -> String {
    match method {
        "razorpay" => "Online (Razorpay)".to_owned(),
        "pay_later" => "Pay later — collect payment manually".to_owned(),
        "upi" => "UPI".to_owned(),
        "card" => "Card".to_owned(),
        "netbanking" => "Netbanking".to_owned(),
        "cod" => "Cash on delivery".to_owned(),
        "bank_transfer" => "Bank transfer".to_owned(),
        other => sanitize_text(other),
    }
}

/// `tel:` target: digits and a leading `+` only, so nothing else can reach the href.
fn tel_href(phone: &str) -> String {
    phone
        .chars()
        .enumerate()
        .filter(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '+'))
        .map(|(_, c)| c)
        .collect()
}

fn html_multiline(text: &str) -> String {
    escape_html(text.trim()).replace('\n', "<br>")
}

/// "₹350", or "FREE" when the order ships free.
pub fn delivery_label(delivery_fee_paise: i64) -> String {
    if delivery_fee_paise > 0 {
        format_inr(delivery_fee_paise)
    } else {
        "FREE".to_owned()
    }
}

/// GST line on order summaries: prices are inclusive, nothing is added.
const TAX_LABEL: &str = "Tax (GST)";
const TAX_AMOUNT: &str = "₹0";

/// "Bulk price (₹10,000+ order)" / "Standard price (order below ₹10,000)".
/// `bulk_min_order_paise` is the currently configured minimum.
pub fn pricing_tier_label(tier: &str, bulk_min_order_paise: i64) -> String {
    let min = format_inr(bulk_min_order_paise);
    match tier {
        "bulk" => format!("Bulk price ({min}+ order)"),
        "standard" => format!("Standard price (order below {min})"),
        other => sanitize_text(other),
    }
}

/// Render the "new order" email for `to`. `public_site_url` (`PUBLIC_SITE_URL`)
/// adds an "Open admin console" button; without it no link is included.
pub fn order_notification(
    order: &Order,
    to: Vec<String>,
    public_site_url: Option<&str>,
    bulk_min_order_paise: i64,
) -> OutboundEmail {
    let c = &order.customer;
    let name_text = sanitize_text(c.name.trim());
    let subject_name: String = name_text.chars().take(MAX_SUBJECT_NAME_CHARS).collect();
    let test_prefix = if order.is_test { "[TEST] " } else { "" };
    let total = format_inr(order.total_amount);
    let subtotal = format_inr(order.subtotal);
    let delivery = delivery_label(order.delivery_fee);
    let mrp = format_inr(order.mrp_total);
    // Savings are on the goods; the delivery charge is not discounted.
    let savings = format_inr((order.mrp_total - order.subtotal).max(0));
    let pricing = pricing_tier_label(&order.pricing_tier, bulk_min_order_paise);
    let order_number = sanitize_text(&order.order_number);
    let placed_at = ist(order.created_at);
    let method = payment_method_label(&order.payment.method);
    let pay_status = sanitize_text(&order.payment.status);
    let rzp_payment = order
        .payment
        .razorpay_payment_id
        .as_deref()
        .map(sanitize_text);
    let instrument = order.payment.instrument.as_deref().map(sanitize_text);
    let email = c
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(sanitize_text);
    let notes = order
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let address = format!(
        "{}, {}, {} {}",
        sanitize_text(c.address.trim()),
        sanitize_text(c.city.trim()),
        sanitize_text(c.state.trim()),
        sanitize_text(c.pincode.trim())
    );
    let console_url = public_site_url
        .map(|u| u.trim().trim_end_matches('/'))
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(|u| format!("{u}/?page=admin"));

    let subject = format!("{test_prefix}New order {order_number} — {total} from {subject_name}");

    // ---- plain text ----
    let mut text = String::with_capacity(1024);
    if order.is_test {
        let _ = writeln!(
            text,
            "*** TEST ORDER (PAYMENT_TEST_MODE: charged ₹1 per unit) ***\n"
        );
    }
    let _ = writeln!(text, "New order {order_number}");
    let _ = writeln!(text, "Placed: {placed_at}\n");
    let _ = writeln!(text, "CUSTOMER");
    let _ = writeln!(text, "  Name:    {name_text}");
    let _ = writeln!(text, "  Phone:   {}", sanitize_text(&c.phone));
    if let Some(e) = &email {
        let _ = writeln!(text, "  Email:   {e}");
    }
    let _ = writeln!(text, "  Address: {address}");
    if let Some(n) = notes {
        let _ = writeln!(text, "  Notes:   {}", n.replace(['\r', '\n'], " "));
    }
    let _ = writeln!(text, "\nITEMS");
    for item in &order.items {
        let _ = writeln!(
            text,
            "  {} x {} ({}) @ {} = {}",
            item.quantity,
            sanitize_text(&item.product_name),
            sanitize_text(&item.pack_info),
            format_inr(item.unit_price),
            format_inr(item.line_total)
        );
    }
    let _ = writeln!(text, "\nMRP total:      {mrp}");
    let _ = writeln!(text, "Savings:        {savings}");
    let _ = writeln!(text, "Subtotal:       {subtotal}");
    let _ = writeln!(text, "Delivery:       {delivery}");
    let _ = writeln!(text, "{TAX_LABEL}:      {TAX_AMOUNT}");
    let _ = writeln!(text, "Amount payable: {total}");
    let _ = writeln!(text, "Pricing:        {pricing}\n");
    let _ = writeln!(text, "PAYMENT");
    let _ = writeln!(text, "  Method: {method}");
    let _ = writeln!(text, "  Status: {pay_status}");
    if let Some(i) = &instrument {
        let _ = writeln!(text, "  Instrument: {i}");
    }
    if let Some(p) = &rzp_payment {
        let _ = writeln!(text, "  Razorpay payment id: {p}");
    }
    if let Some(url) = &console_url {
        let _ = writeln!(text, "\nOpen admin console: {url}");
    }

    // ---- HTML ----
    const LABEL: &str = "padding:4px 12px 4px 0;color:#94a3b8;font-size:13px;vertical-align:top;white-space:nowrap;";
    const VALUE: &str = "padding:4px 0;color:#e2e8f0;font-size:14px;";
    const TH: &str = "padding:8px 6px;color:#94a3b8;font-size:12px;font-weight:600;border-bottom:1px solid #334155;";
    const TD: &str = "padding:8px 6px;color:#e2e8f0;font-size:13px;border-bottom:1px solid #1f2a3d;vertical-align:top;";

    let mut customer_rows = String::new();
    let mut row = |label: &str, value_html: String| {
        let _ = write!(
            customer_rows,
            r#"<tr><td style="{LABEL}">{label}</td><td style="{VALUE}">{value_html}</td></tr>"#
        );
    };
    row("Name", escape_html(&name_text));
    row(
        "Phone",
        format!(
            r#"<a href="tel:{}" style="color:#fbbf24;text-decoration:none;">{}</a>"#,
            escape_html(&tel_href(&c.phone)),
            escape_html(&c.phone)
        ),
    );
    if let Some(e) = &email {
        row("Email", escape_html(e));
    }
    row("Address", escape_html(&address));
    if let Some(n) = notes {
        row("Notes", html_multiline(n));
    }

    let mut item_rows = String::new();
    for item in &order.items {
        let _ = write!(
            item_rows,
            r#"<tr><td style="{TD}">{}</td><td style="{TD}">{}</td><td style="{TD}text-align:right;">{}</td><td style="{TD}text-align:right;white-space:nowrap;">{}</td><td style="{TD}text-align:right;white-space:nowrap;">{}</td></tr>"#,
            escape_html(&item.product_name),
            escape_html(&item.pack_info),
            item.quantity,
            escape_html(&format_inr(item.unit_price)),
            escape_html(&format_inr(item.line_total)),
        );
    }

    let mut payment_rows = String::new();
    let mut prow = |label: &str, value: &str| {
        let _ = write!(
            payment_rows,
            r#"<tr><td style="{LABEL}">{label}</td><td style="{VALUE}">{}</td></tr>"#,
            escape_html(value)
        );
    };
    prow("Method", &method);
    prow("Status", &pay_status);
    if let Some(i) = &instrument {
        prow("Instrument", i);
    }
    if let Some(p) = &rzp_payment {
        prow("Razorpay payment id", p);
    }

    let test_banner = if order.is_test {
        r#"<tr><td style="padding:10px 32px;background-color:#451a03;color:#fde68a;font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:13px;font-weight:700;">TEST ORDER — PAYMENT_TEST_MODE (charged ₹1 per unit). Not real revenue.</td></tr>"#
    } else {
        ""
    };
    let button = match &console_url {
        Some(url) => format!(
            r#"<tr><td align="center" style="padding:8px 32px 28px 32px;"><table role="presentation" cellpadding="0" cellspacing="0" border="0"><tr><td style="background-color:#f59e0b;border-radius:8px;"><a href="{href}" style="display:inline-block;padding:12px 28px;font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:15px;font-weight:700;color:#0f172a;text-decoration:none;">Open admin console</a></td></tr></table></td></tr>"#,
            href = escape_html(url)
        ),
        None => String::new(),
    };

    let mut html = String::with_capacity(8192);
    let _ = write!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>{subject_html}</title>
</head>
<body style="margin:0;padding:0;background-color:#0f172a;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">{order_number_html} — {total_html} from {name_html}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background-color:#0f172a;">
<tr><td align="center" style="padding:32px 12px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:640px;background-color:#1e293b;border:1px solid #334155;border-radius:12px;">
<tr><td style="padding:24px 32px;border-bottom:3px solid #f59e0b;">
<span style="font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:20px;font-weight:700;color:#f59e0b;">Zhi-Mart</span>
<span style="font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:20px;color:#e2e8f0;"> Admin</span>
</td></tr>
{test_banner}
<tr><td style="padding:24px 32px 4px 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<div style="font-size:13px;color:#94a3b8;">New order</div>
<div style="font-size:24px;font-weight:700;color:#f8fafc;font-family:Consolas,Menlo,monospace;">{order_number_html}</div>
<div style="font-size:13px;color:#94a3b8;padding-top:4px;">{placed_html}</div>
<div style="font-size:28px;font-weight:700;color:#fbbf24;padding-top:12px;">{total_html}</div>
</td></tr>
<tr><td style="padding:16px 32px 0 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<div style="font-size:12px;font-weight:700;letter-spacing:1px;color:#f59e0b;padding-bottom:6px;">CUSTOMER</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="font-family:Segoe UI,Helvetica,Arial,sans-serif;">{customer_rows}</table>
</td></tr>
<tr><td style="padding:20px 32px 0 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<div style="font-size:12px;font-weight:700;letter-spacing:1px;color:#f59e0b;padding-bottom:6px;">ITEMS</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<tr><th align="left" style="{TH}">Product</th><th align="left" style="{TH}">Pack</th><th align="right" style="{TH}">Qty</th><th align="right" style="{TH}">Unit price</th><th align="right" style="{TH}">Line total</th></tr>
{item_rows}
</table>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="font-family:Segoe UI,Helvetica,Arial,sans-serif;margin-top:8px;">
<tr><td style="padding:3px 0;color:#94a3b8;font-size:13px;">MRP total</td><td style="padding:3px 0;color:#94a3b8;font-size:13px;text-align:right;text-decoration:line-through;">{mrp_html}</td></tr>
<tr><td style="padding:3px 0;color:#86efac;font-size:13px;">Savings</td><td style="padding:3px 0;color:#86efac;font-size:13px;text-align:right;">{savings_html}</td></tr>
<tr><td style="padding:3px 0;color:#e2e8f0;font-size:13px;">Subtotal</td><td style="padding:3px 0;color:#e2e8f0;font-size:13px;text-align:right;">{subtotal_html}</td></tr>
<tr><td style="padding:3px 0;color:#e2e8f0;font-size:13px;">Delivery</td><td style="padding:3px 0;color:#e2e8f0;font-size:13px;text-align:right;">{delivery_html}</td></tr>
<tr><td style="padding:3px 0;color:#e2e8f0;font-size:13px;">{tax_label}</td><td style="padding:3px 0;color:#e2e8f0;font-size:13px;text-align:right;">{tax_amount}</td></tr>
<tr><td style="padding:6px 0;color:#f8fafc;font-size:15px;font-weight:700;">Amount payable</td><td style="padding:6px 0;color:#fbbf24;font-size:15px;font-weight:700;text-align:right;">{total_html}</td></tr>
<tr><td style="padding:3px 0;color:#94a3b8;font-size:13px;">Pricing</td><td style="padding:3px 0;color:#94a3b8;font-size:13px;text-align:right;">{pricing_html}</td></tr>
</table>
</td></tr>
<tr><td style="padding:20px 32px 16px 32px;font-family:Segoe UI,Helvetica,Arial,sans-serif;">
<div style="font-size:12px;font-weight:700;letter-spacing:1px;color:#f59e0b;padding-bottom:6px;">PAYMENT</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="font-family:Segoe UI,Helvetica,Arial,sans-serif;">{payment_rows}</table>
</td></tr>
{button}
<tr><td style="padding:16px 32px;border-top:1px solid #334155;font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:12px;line-height:18px;color:#64748b;">
Automated notification from {brand}. Customer details are confidential — do not forward this email.
</td></tr>
</table>
</td></tr>
</table>
</body>
</html>
"#,
        subject_html = escape_html(&subject),
        order_number_html = escape_html(&order_number),
        total_html = escape_html(&total),
        name_html = escape_html(&name_text),
        placed_html = escape_html(&placed_at),
        mrp_html = escape_html(&mrp),
        savings_html = escape_html(&savings),
        subtotal_html = escape_html(&subtotal),
        delivery_html = escape_html(&delivery),
        tax_label = escape_html(TAX_LABEL),
        tax_amount = escape_html(TAX_AMOUNT),
        pricing_html = escape_html(&pricing),
        brand = escape_html(BRAND),
    );

    OutboundEmail {
        to,
        subject,
        html,
        text,
    }
}

// ---------------------------------------------------------------------------
// Customer "order confirmed" email (with the private tracking link)
// ---------------------------------------------------------------------------

/// Basic shape check for a stored customer address; anything odd is not sent.
pub fn is_plausible_email(addr: &str) -> bool {
    let addr = addr.trim();
    (3..=254).contains(&addr.len())
        && !addr.chars().any(|c| {
            c.is_whitespace() || c.is_control() || c == ',' || c == ';' || c == '<' || c == '>'
        })
        && addr.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && domain.contains('.') && !domain.contains('@')
        })
}

/// `+91 ••••• •1234`: only the last 4 digits are shown in emails.
pub fn mask_phone(phone: &str) -> String {
    let digits: Vec<char> = phone.chars().filter(char::is_ascii_digit).collect();
    let last4: String = digits[digits.len().saturating_sub(4)..].iter().collect();
    if phone.trim_start().starts_with("+91") {
        format!("+91 ••••• •{last4}")
    } else {
        format!("•••••• {last4}")
    }
}

fn instrument_label(instrument: Option<&str>) -> String {
    match instrument {
        Some("upi") => "UPI".to_owned(),
        Some("card") => "card".to_owned(),
        Some("netbanking") => "netbanking".to_owned(),
        Some("wallet") => "wallet".to_owned(),
        Some("emi") => "EMI".to_owned(),
        Some("paylater") => "pay later".to_owned(),
        Some("bank_transfer") => "bank transfer".to_owned(),
        Some(other) => sanitize_text(other),
        None => "online payment".to_owned(),
    }
}

/// Render the customer's confirmation for `order`. `None` when the order has
/// no usable customer email. The tracking link carries the order token in
/// the URL fragment: never log the returned bodies.
pub fn customer_order_confirmation(order: &Order, config: &Config) -> Option<OutboundEmail> {
    let to = order
        .customer
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| is_plausible_email(e))?;
    let c = &order.customer;
    let token = order_token::issue(config, order.id);
    let tracking_url = config.order_tracking_url(&order.order_number, &token);

    let name_text = sanitize_text(c.name.trim());
    let order_number = sanitize_text(&order.order_number);
    let placed_at = ist(order.created_at);
    let total = format_inr(order.total_amount);
    let subtotal = format_inr(order.subtotal);
    let delivery = delivery_label(order.delivery_fee);
    let mrp = format_inr(order.mrp_total);
    // Savings are on the goods; the delivery charge is not discounted.
    let savings_paise = (order.mrp_total - order.subtotal).max(0);
    let savings = format_inr(savings_paise);
    let pricing = pricing_tier_label(&order.pricing_tier, config.pricing.bulk_min_order_paise);
    let store = sanitize_text(config.store_name.trim());
    let test_prefix = if order.is_test { "[TEST] " } else { "" };
    let subject = format!("{test_prefix}Your Zhi-Mart order {order_number} is confirmed");
    let paid = order.payment.status == "paid";
    let payment_line = if paid {
        format!(
            "Payment of {total} received via {}.",
            instrument_label(order.payment.instrument.as_deref())
        )
    } else {
        format!(
            "Our team will call you on {} to confirm payment and dispatch.",
            mask_phone(&c.phone)
        )
    };
    let address_lines = [
        name_text.clone(),
        sanitize_text(c.address.trim()),
        format!(
            "{}, {}",
            sanitize_text(c.city.trim()),
            sanitize_text(c.state.trim())
        ),
        format!("PIN {}", sanitize_text(c.pincode.trim())),
    ];
    const PRIVACY: &str = "Keep this link private — anyone with it can see your order status.";

    // ---- plain text ----
    let mut text = String::with_capacity(1024);
    if order.is_test {
        let _ = writeln!(text, "*** TEST ORDER — not a real purchase ***\n");
    }
    let _ = writeln!(text, "Hi {name_text},\n");
    let _ = writeln!(text, "Thank you! Your order {order_number} is confirmed.");
    let _ = writeln!(text, "Placed: {placed_at}\n");
    let _ = writeln!(text, "Track your order:\n{tracking_url}\n{PRIVACY}\n");
    let _ = writeln!(text, "ITEMS");
    for item in &order.items {
        let _ = writeln!(
            text,
            "  {} ({}) — {} x {} = {}",
            sanitize_text(&item.product_name),
            sanitize_text(&item.pack_info),
            item.quantity,
            format_inr(item.unit_price),
            format_inr(item.line_total)
        );
    }
    let _ = writeln!(text, "\nMRP total: {mrp}");
    if savings_paise > 0 {
        let _ = writeln!(text, "You save:  {savings}");
    }
    let _ = writeln!(text, "Subtotal:  {subtotal}");
    let _ = writeln!(text, "Delivery:  {delivery}");
    let _ = writeln!(text, "{TAX_LABEL}: {TAX_AMOUNT}");
    let _ = writeln!(text, "Total:     {total}");
    let _ = writeln!(text, "Pricing:   {pricing}\n");
    let _ = writeln!(text, "PAYMENT\n  {payment_line}\n");
    let _ = writeln!(text, "DELIVERY ADDRESS");
    for line in &address_lines {
        let _ = writeln!(text, "  {line}");
    }
    let _ = writeln!(
        text,
        "\nThis is an automated message from {store}; this mailbox is not monitored."
    );

    // ---- HTML (single column, ≥14px text, large tap targets) ----
    const FONT: &str = "font-family:Segoe UI,Helvetica,Arial,sans-serif;";
    const SECTION: &str =
        "font-size:13px;font-weight:700;letter-spacing:1px;color:#f59e0b;padding-bottom:8px;";
    let mut items_html = String::new();
    for item in &order.items {
        let _ = write!(
            items_html,
            r#"<tr><td style="padding:10px 0;border-bottom:1px solid #334155;{FONT}">
<div style="font-size:15px;font-weight:600;color:#f8fafc;">{name}</div>
<div style="font-size:14px;color:#94a3b8;padding-top:2px;">{pack} · {qty} × {unit}</div>
</td><td style="padding:10px 0 10px 12px;border-bottom:1px solid #334155;text-align:right;vertical-align:top;white-space:nowrap;font-size:15px;color:#e2e8f0;{FONT}">{line}</td></tr>"#,
            name = escape_html(&item.product_name),
            pack = escape_html(&item.pack_info),
            qty = item.quantity,
            unit = escape_html(&format_inr(item.unit_price)),
            line = escape_html(&format_inr(item.line_total)),
        );
    }
    let savings_row = if savings_paise > 0 {
        format!(
            r#"<tr><td style="padding:4px 0;font-size:14px;color:#86efac;{FONT}">You save</td><td style="padding:4px 0;font-size:14px;color:#86efac;text-align:right;{FONT}">{}</td></tr>"#,
            escape_html(&savings)
        )
    } else {
        String::new()
    };
    let address_html = address_lines
        .iter()
        .map(|l| escape_html(l))
        .collect::<Vec<_>>()
        .join("<br>");
    let test_banner = if order.is_test {
        format!(
            r#"<tr><td style="padding:12px 24px;background-color:#451a03;color:#fde68a;font-size:14px;font-weight:700;{FONT}">TEST ORDER — not a real purchase.</td></tr>"#
        )
    } else {
        String::new()
    };
    let href = escape_html(&tracking_url);

    let mut html = String::with_capacity(8192);
    let _ = write!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>{subject_html}</title>
</head>
<body style="margin:0;padding:0;background-color:#0f172a;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">Order {order_number_html} confirmed — {total_html}. Track it anytime.</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background-color:#0f172a;">
<tr><td align="center" style="padding:20px 8px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:600px;background-color:#1e293b;border:1px solid #334155;border-radius:12px;">
<tr><td style="padding:20px 24px;border-bottom:3px solid #f59e0b;{FONT}font-size:22px;font-weight:700;color:#f59e0b;">{store_html}</td></tr>
{test_banner}
<tr><td style="padding:24px 24px 8px 24px;{FONT}color:#e2e8f0;font-size:16px;line-height:24px;">
<p style="margin:0 0 12px 0;">Hi {name_html},</p>
<p style="margin:0 0 4px 0;font-size:20px;font-weight:700;color:#f8fafc;">Your order is confirmed 🎉</p>
<p style="margin:0;font-size:15px;color:#94a3b8;">Order <strong style="color:#f8fafc;font-family:Consolas,Menlo,monospace;">{order_number_html}</strong> · {placed_html}</p>
</td></tr>
<tr><td align="center" style="padding:20px 24px 8px 24px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td align="center" style="background-color:#f59e0b;border-radius:10px;">
<a href="{href}" style="display:block;padding:16px 24px;{FONT}font-size:18px;font-weight:700;color:#0f172a;text-decoration:none;">Track your order</a>
</td></tr></table>
</td></tr>
<tr><td style="padding:8px 24px 4px 24px;{FONT}font-size:14px;line-height:20px;color:#94a3b8;word-break:break-all;">
If the button doesn't work, open this link:<br><a href="{href}" style="color:#fbbf24;">{href}</a>
</td></tr>
<tr><td style="padding:8px 24px 16px 24px;{FONT}font-size:14px;line-height:20px;color:#fde68a;">🔒 {privacy_html}</td></tr>
<tr><td style="padding:8px 24px 0 24px;{FONT}">
<div style="{SECTION}">ITEMS</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">{items_html}</table>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin-top:8px;">
<tr><td style="padding:4px 0;font-size:14px;color:#94a3b8;{FONT}">MRP total</td><td style="padding:4px 0;font-size:14px;color:#94a3b8;text-align:right;text-decoration:line-through;{FONT}">{mrp_html}</td></tr>
{savings_row}
<tr><td style="padding:4px 0;font-size:14px;color:#e2e8f0;{FONT}">Subtotal</td><td style="padding:4px 0;font-size:14px;color:#e2e8f0;text-align:right;{FONT}">{subtotal_html}</td></tr>
<tr><td style="padding:4px 0;font-size:14px;color:#e2e8f0;{FONT}">Delivery</td><td style="padding:4px 0;font-size:14px;color:#e2e8f0;text-align:right;{FONT}">{delivery_html}</td></tr>
<tr><td style="padding:4px 0;font-size:14px;color:#e2e8f0;{FONT}">{tax_label}</td><td style="padding:4px 0;font-size:14px;color:#e2e8f0;text-align:right;{FONT}">{tax_amount}</td></tr>
<tr><td style="padding:8px 0;font-size:18px;font-weight:700;color:#f8fafc;{FONT}">Total</td><td style="padding:8px 0;font-size:18px;font-weight:700;color:#fbbf24;text-align:right;{FONT}">{total_html}</td></tr>
<tr><td colspan="2" style="padding:2px 0;font-size:14px;color:#94a3b8;{FONT}">Pricing: {pricing_html}</td></tr>
</table>
</td></tr>
<tr><td style="padding:16px 24px 0 24px;{FONT}">
<div style="{SECTION}">PAYMENT</div>
<div style="font-size:15px;line-height:22px;color:#e2e8f0;">{payment_html}</div>
</td></tr>
<tr><td style="padding:16px 24px 20px 24px;{FONT}">
<div style="{SECTION}">DELIVERY ADDRESS</div>
<div style="font-size:15px;line-height:22px;color:#e2e8f0;">{address_html}</div>
</td></tr>
<tr><td style="padding:16px 24px;border-top:1px solid #334155;{FONT}font-size:13px;line-height:19px;color:#64748b;">
This is an automated message from {store_html}; this mailbox is not monitored.
</td></tr>
</table>
</td></tr>
</table>
</body>
</html>
"#,
        subject_html = escape_html(&subject),
        order_number_html = escape_html(&order_number),
        total_html = escape_html(&total),
        store_html = escape_html(&store),
        name_html = escape_html(&name_text),
        placed_html = escape_html(&placed_at),
        privacy_html = escape_html(PRIVACY),
        mrp_html = escape_html(&mrp),
        subtotal_html = escape_html(&subtotal),
        delivery_html = escape_html(&delivery),
        tax_label = escape_html(TAX_LABEL),
        tax_amount = escape_html(TAX_AMOUNT),
        pricing_html = escape_html(&pricing),
        payment_html = escape_html(&payment_line),
    );

    Some(OutboundEmail {
        to: vec![to.to_owned()],
        subject,
        html,
        text,
    })
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    fn sample(name: &str, ip: Option<&str>) -> OtpEmail {
        OtpEmail {
            to: "admin@example.com".into(),
            display_name: name.into(),
            code: SecretString::from("042917"),
            expires_in_minutes: 5,
            request_ip: ip.map(|s| s.parse().expect("ip")),
        }
    }

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-13T08:30:00Z")
            .expect("date")
            .with_timezone(&Utc)
    }

    #[test]
    fn contains_code_validity_ip_and_warning() {
        let r = render_otp_email(&sample("Ops", Some("203.0.113.9")), at());
        assert!(
            !r.subject.contains("042917"),
            "code must not be in the subject"
        );
        for body in [r.html.as_str(), r.text.as_str()] {
            assert!(body.contains("042917"));
            assert!(body.contains("5 minutes"));
            assert!(body.contains("203.0.113.9"));
            assert!(body.contains("your password may be compromised — reset it immediately"));
            assert!(body.contains("13 Oct 2026, 08:30 UTC"));
        }
        assert!(!r.html.contains("<img"));
        assert!(
            !r.html.contains("http://") && !r.html.contains("https://"),
            "no remote content"
        );
    }

    #[test]
    fn escapes_display_name() {
        let r = render_otp_email(&sample("<script>alert('x')</script> & \"co\"", None), at());
        assert!(!r.html.contains("<script>"));
        assert!(
            r.html.contains(
                "&lt;script&gt;alert(&#39;x&#39;)&lt;&#x2F;script&gt; &amp; &quot;co&quot;"
            )
        );
        assert!(!r.html.contains("From IP address"));
        assert!(!r.text.contains("From IP address"));
    }

    #[test]
    fn strips_control_characters_from_text_part() {
        let r = render_otp_email(&sample("Eve\r\nBcc: x@y", None), at());
        assert!(r.text.contains("Hi EveBcc: x@y,"));
    }

    #[test]
    fn singular_minute_and_unspecified_ip() {
        let mut e = sample("", Some("0.0.0.0"));
        e.expires_in_minutes = 1;
        let r = render_otp_email(&e, at());
        assert!(r.text.contains("1 minute and"));
        assert!(r.text.contains("Hi there,"));
        assert!(!r.text.contains("0.0.0.0"));
    }

    // ---- order notification ----

    use crate::repo::orders::{Customer, Order, OrderItem, Payment, Shipping};

    fn order(is_test: bool) -> Order {
        Order {
            id: uuid::Uuid::now_v7(),
            order_number: "ZM-261013-1001".into(),
            created_at: at(),
            updated_at: at(),
            version: 1,
            customer: Customer {
                name: "<script>alert(1)</script> \"Ravi\" & Sons".into(),
                phone: "+919840012345".into(),
                email: Some("ravi&co@example.com".into()),
                address: "12 <b>Gandhi</b> St".into(),
                city: "Sivakasi".into(),
                state: "Tamil Nadu".into(),
                pincode: "626123".into(),
            },
            notes: Some("Call before\n<img src=x onerror=alert(1)>".into()),
            items: vec![
                OrderItem {
                    product_id: "zm-01".into(),
                    product_name: "Flower Pots <Deluxe>".into(),
                    pack_info: "Box of 10 'big'".into(),
                    quantity: 3,
                    unit_price: 16_500,
                    unit_mrp: 30_000,
                    line_total: 49_500,
                },
                OrderItem {
                    product_id: "zm-02".into(),
                    product_name: "Chakra".into(),
                    pack_info: "Box of 25".into(),
                    quantity: 1,
                    unit_price: 12_345_650,
                    unit_mrp: 20_000_000,
                    line_total: 12_345_650,
                },
            ],
            item_count: 4,
            mrp_total: 20_090_000,
            subtotal: 12_395_150,
            delivery_fee: 0,
            total_amount: 12_395_150,
            status: "pending_pack".into(),
            payment: Payment {
                method: "razorpay".into(),
                status: "paid".into(),
                reference: None,
                paid_at: Some(at()),
                refunded_at: None,
                instrument: Some("upi".into()),
                razorpay_order_id: Some("order_ABC123".into()),
                razorpay_payment_id: Some("pay_XYZ789".into()),
                amount_refunded: 0,
                attention: None,
            },
            shipping: Shipping {
                transport_partner: None,
                docket_number: None,
                dispatched_at: None,
                delivered_at: None,
            },
            cancelled_at: None,
            cancel_reason: None,
            is_test,
            payment_expires_at: None,
            pricing_tier: "bulk".into(),
        }
    }

    #[test]
    fn inr_formatting_uses_indian_grouping() {
        assert_eq!(format_inr(0), "₹0");
        assert_eq!(format_inr(100), "₹1");
        assert_eq!(format_inr(165_050), "₹1,650.50");
        assert_eq!(format_inr(99_999_900), "₹9,99,999");
        assert_eq!(format_inr(12_345_678_900), "₹12,34,56,789");
        assert_eq!(format_inr(123_456_700), "₹12,34,567");
        assert_eq!(format_inr(100_000), "₹1,000");
        assert_eq!(format_inr(-5_000), "-₹50");
    }

    #[test]
    fn order_email_content_and_totals() {
        let to = vec!["ops@zhimart.com".to_owned()];
        let e = order_notification(
            &order(false),
            to.clone(),
            Some("https://zhimart.com/"),
            1_000_000,
        );
        assert_eq!(e.to, to);
        assert!(
            e.subject
                .starts_with("New order ZM-261013-1001 — ₹1,23,951.50 from ")
        );
        assert!(!e.subject.contains('\n'));
        for body in [&e.html, &e.text] {
            assert!(body.contains("ZM-261013-1001"));
            assert!(body.contains("13 Oct 2026, 02:00 PM IST"), "IST time");
            assert!(body.contains("₹2,00,900"), "MRP total");
            assert!(body.contains("₹76,948.50"), "savings");
            assert!(body.contains("₹1,23,951.50"), "payable");
            assert!(body.contains("₹165"), "unit price");
            assert!(body.contains("₹495"), "line total");
            assert!(body.contains("pay_XYZ789"));
            assert!(body.contains("Online (Razorpay)"));
            assert!(body.contains("Bulk price (₹10,000+ order)"), "pricing tier");
            assert!(body.contains("626123"));
            assert!(body.contains("Subtotal") && body.contains("₹1,23,951.50"));
            assert!(body.contains("FREE"), "free delivery");
            assert!(body.contains("Tax (GST)") && body.contains("₹0"));
        }
        assert!(e.text.contains("Delivery:       FREE"));
        assert!(e.text.contains("Tax (GST):      ₹0"));
        assert!(e.text.contains("https://zhimart.com/?page=admin"));
        let href = format!(
            r#"href="{}""#,
            escape_html("https://zhimart.com/?page=admin")
        );
        assert!(
            e.html.contains(&href),
            "console link (entity-encoded in the attribute)"
        );
        assert!(e.html.contains(r#"href="tel:+919840012345""#));
        assert!(e.html.contains("Open admin console"));
        assert!(!e.html.contains("<img") && !e.html.contains("http://"));
    }

    /// A small standard-tier order: ₹495 of goods + ₹350 delivery.
    fn small_order() -> Order {
        let mut o = order(false);
        o.items.truncate(1);
        o.item_count = 3;
        o.mrp_total = 90_000;
        o.subtotal = 49_500;
        o.delivery_fee = 35_000;
        o.total_amount = 84_500;
        o.pricing_tier = "standard".into();
        o.customer.email = Some("ravi@example.com".into());
        o
    }

    #[test]
    fn order_emails_show_subtotal_delivery_tax_and_total() {
        let o = small_order();
        let admin = order_notification(&o, vec!["a@b.co".into()], None, 1_000_000);
        let customer = customer_order_confirmation(&o, &cfg()).expect("rendered");
        for body in [&admin.html, &admin.text, &customer.html, &customer.text] {
            assert!(body.contains("₹495"), "subtotal");
            assert!(body.contains("₹350"), "delivery fee");
            assert!(body.contains("₹845"), "total incl. delivery");
            assert!(body.contains("₹405"), "savings on goods only (₹900 − ₹495)");
            assert!(body.contains("Tax (GST)"));
            assert!(!body.contains("FREE"));
        }
        assert!(admin.subject.contains("₹845"));
        assert!(admin.text.contains("Subtotal:       ₹495"));
        assert!(admin.text.contains("Delivery:       ₹350"));
        assert!(admin.text.contains("Amount payable: ₹845"));
        assert!(customer.text.contains("Subtotal:  ₹495"));
        assert!(customer.text.contains("Delivery:  ₹350"));
        assert!(customer.text.contains("Tax (GST): ₹0"));
        assert!(customer.text.contains("Total:     ₹845"));
        assert!(customer.text.contains("Payment of ₹845 received via UPI"));
    }

    #[test]
    fn order_email_escapes_every_customer_value() {
        let e = order_notification(&order(false), vec!["a@b.co".into()], None, 1_000_000);
        assert!(!e.html.contains("<script>"), "script tag escaped");
        assert!(!e.html.contains("<b>Gandhi"), "address escaped");
        assert!(!e.html.contains("<img src=x"), "notes escaped");
        assert!(!e.html.contains("<Deluxe>"), "product escaped");
        assert!(
            e.html
                .contains("&lt;script&gt;alert(1)&lt;&#x2F;script&gt; &quot;Ravi&quot; &amp; Sons")
        );
        assert!(e.html.contains("ravi&amp;co@example.com"));
        assert!(e.html.contains("Box of 10 &#39;big&#39;"));
        assert!(e.html.contains("Call before<br>&lt;img"));
    }

    #[test]
    fn order_email_test_prefix_and_no_link_without_site_url() {
        let e = order_notification(&order(true), vec!["a@b.co".into()], None, 1_000_000);
        assert!(e.subject.starts_with("[TEST] New order ZM-261013-1001"));
        assert!(e.html.contains("TEST ORDER"));
        assert!(e.text.contains("TEST ORDER"));
        assert!(!e.html.contains("Open admin console"));
        assert!(!e.html.contains("href=\"http"));
        assert!(!e.text.contains("admin console"));

        let mut later = order(false);
        later.payment.method = "pay_later".into();
        later.payment.status = "pending".into();
        later.payment.razorpay_payment_id = None;
        later.payment.instrument = None;
        later.customer.email = None;
        let e = order_notification(
            &later,
            vec!["a@b.co".into()],
            Some("javascript:alert(1)"),
            1_000_000,
        );
        assert!(e.text.contains("Pay later"));
        assert!(!e.text.contains("Razorpay payment id"));
        assert!(!e.html.contains("javascript:"), "non-http site URL ignored");
        assert!(!e.text.contains("Email:"));
    }

    // ---- customer confirmation ----

    fn cfg() -> crate::config::Config {
        let mut c = crate::auth::jwt::tests::test_config();
        c.public_site_url = Some("https://zhimart.example".into());
        c
    }

    #[test]
    fn phone_masking_and_email_shape() {
        assert_eq!(mask_phone("+919840012345"), "+91 ••••• •2345");
        assert_eq!(mask_phone("9840012345"), "•••••• 2345");
        assert!(is_plausible_email("a.b@example.co"));
        for bad in [
            "",
            "nope",
            "a@b",
            "a b@x.co",
            "a@x.co, b@y.co",
            "<a@x.co>",
            "a@@x.co",
        ] {
            assert!(!is_plausible_email(bad), "{bad}");
        }
    }

    #[test]
    fn customer_email_tracking_link_and_escaping() {
        let config = cfg();
        let mut o = order(false);
        o.customer.email = Some("  ravi@example.com ".into());
        let e = customer_order_confirmation(&o, &config).expect("rendered");
        assert_eq!(
            e.to,
            vec!["ravi@example.com".to_owned()],
            "single recipient"
        );
        assert_eq!(e.subject, "Your Zhi-Mart order ZM-261013-1001 is confirmed");

        let token = crate::payments::order_token::issue(&config, o.id);
        let url = format!("https://zhimart.example/?page=order&order=ZM-261013-1001#t={token}");
        assert!(crate::payments::order_token::verify(&config, o.id, &token));
        assert!(e.text.contains(&url), "raw link in text");
        assert!(
            e.html.contains(&format!(r#"href="{}""#, escape_html(&url))),
            "button href"
        );
        assert!(e.html.contains("Track your order"));
        assert!(e.html.contains("anyone with it can see your order status"));

        assert!(!e.html.contains("<script>") && !e.html.contains("<Deluxe>"));
        assert!(
            e.html
                .contains("&lt;script&gt;alert(1)&lt;&#x2F;script&gt; &quot;Ravi&quot; &amp; Sons")
        );
        assert!(e.html.contains("12 &lt;b&gt;Gandhi&lt;&#x2F;b&gt; St"));
        for body in [&e.html, &e.text] {
            assert!(body.contains("₹1,23,951.50"));
            assert!(body.contains("₹76,948.50"), "savings");
            assert!(body.contains("₹2,00,900"), "MRP");
            assert!(body.contains("PIN 626123"));
            assert!(body.contains("Payment of ₹1,23,951.50 received via UPI"));
            assert!(
                body.contains("Subtotal") && body.contains("FREE"),
                "free delivery"
            );
            assert!(body.contains("Bulk price (₹10,000+ order)"), "pricing tier");
            assert!(!body.contains("9840012345"), "full phone never shown");
            assert!(
                !body.contains("pay_XYZ789"),
                "gateway ids not shown to customers"
            );
        }
    }

    #[test]
    fn customer_email_pay_later_test_prefix_and_missing_email() {
        let config = cfg();
        let mut o = order(true);
        o.customer.email = Some("ravi@example.com".into());
        o.payment.method = "pay_later".into();
        o.payment.status = "pending".into();
        let e = customer_order_confirmation(&o, &config).expect("rendered");
        assert!(e.subject.starts_with("[TEST] Your Zhi-Mart order"));
        assert!(e.text.contains(
            "Our team will call you on +91 ••••• •2345 to confirm payment and dispatch."
        ));
        assert!(e.html.contains("TEST ORDER"));
        assert!(!e.text.contains("received via"));

        o.pricing_tier = "standard".into();
        let e = customer_order_confirmation(&o, &config).expect("rendered");
        for body in [&e.html, &e.text] {
            assert!(body.contains("Standard price (order below ₹10,000)"));
        }

        o.customer.email = None;
        assert!(customer_order_confirmation(&o, &config).is_none());
        o.customer.email = Some("not an email".into());
        assert!(customer_order_confirmation(&o, &config).is_none());
    }
}
