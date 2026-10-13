-- Customer order-confirmation emails (with the private tracking link) go
-- through the same transactional outbox as the admin notifications.
ALTER TABLE email_outbox DROP CONSTRAINT email_outbox_kind;
ALTER TABLE email_outbox ADD CONSTRAINT email_outbox_kind CHECK (
    kind IN ('admin_order_notification', 'customer_order_confirmation')
);
