/**
 * Admin console contract types — mirror `backend/API.md` ("Catalog, Inventory & Orders").
 * Money is in rupees (numbers, up to 2 decimals); timestamps are RFC 3339 UTC strings.
 */

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

export const ORDER_STATUSES = [
  'pending_payment',
  'pending_pack',
  'dispatched',
  'in_transit',
  'out_for_delivery',
  'delivered',
  'cancelled',
] as const;
export type OrderStatus = (typeof ORDER_STATUSES)[number];

/** Forward fulfilment pipeline (excludes `pending_payment`, which only payment can advance, and `cancelled`). */
export const ORDER_PIPELINE: readonly OrderStatus[] = [
  'pending_pack',
  'dispatched',
  'in_transit',
  'out_for_delivery',
  'delivered',
];

export const PAYMENT_STATUSES = ['pending', 'paid', 'refunded', 'failed'] as const;
export type PaymentStatus = (typeof PAYMENT_STATUSES)[number];

export const PAYMENT_METHODS = ['razorpay', 'pay_later', 'upi', 'card', 'netbanking', 'cod', 'bank_transfer'] as const;
export type PaymentMethod = (typeof PAYMENT_METHODS)[number];

/** Methods an admin may record with `mark_paid` (legacy manual collection). */
export const MANUAL_PAYMENT_METHODS = ['upi', 'card', 'netbanking', 'cod', 'bank_transfer'] as const;

export type PaymentAction = 'mark_paid' | 'mark_failed' | 'mark_refunded';

export const SOUND_LEVELS = ['Low Sound', 'Single Loud Blast', 'Repetitive Blast', 'Silent & Visual'] as const;
export type SoundLevel = (typeof SOUND_LEVELS)[number];

export type InventoryStatus = 'in_stock' | 'low_stock' | 'out_of_stock';

export type StockAdjustMode = 'set' | 'add' | 'remove';

export type StockMovementReason =
  | 'initial'
  | 'order_placed'
  | 'order_cancelled'
  | 'payment_expired'
  | 'payment_revived'
  | 'manual_set'
  | 'manual_adjust'
  | 'restock';

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

export interface AdminProduct {
  id: string;
  name: string;
  tamilName: string | null;
  category: string;
  brand: string;
  packInfo: string;
  piecesPerPack: number | null;
  /** Price-list (bulk) price — charged when the order reaches the bulk threshold (≈85% off MRP). */
  price: number;
  originalPrice: number;
  discountPercent: number;
  /** Price charged below the bulk threshold (≈75% off MRP; never below `price`). Derived by the server. */
  standardPrice?: number;
  standardDiscountPercent?: number;
  image: string;
  gallery: string[];
  description: string;
  soundLevel: SoundLevel | null;
  isPopular: boolean;
  isBestSelling: boolean;
  isNewArrival: boolean;
  isParty: boolean;
  inStock: boolean;
  availableQuantity: number;
  stock: number;
  lowStockThreshold: number;
  bayLocation: string | null;
  isActive: boolean;
  sortOrder: number;
  version: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProductInput {
  /** Create only; generated from the name when omitted. */
  id?: string;
  name: string;
  tamilName: string | null;
  category: string;
  brand: string;
  packInfo: string;
  piecesPerPack: number | null;
  price: number;
  originalPrice: number;
  image: string;
  gallery: string[];
  description: string;
  soundLevel: SoundLevel | null;
  isPopular: boolean;
  isBestSelling: boolean;
  isNewArrival: boolean;
  isParty: boolean;
  lowStockThreshold: number;
  bayLocation: string | null;
  sortOrder: number;
  /** Create only. */
  initialStock?: number;
  /** Update only (required). */
  expectedVersion?: number;
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

export interface InventoryItem {
  productId: string;
  name: string;
  category: string;
  image: string;
  stock: number;
  lowStockThreshold: number;
  bayLocation: string | null;
  isActive: boolean;
  status: InventoryStatus;
  version: number;
  updatedAt: string;
}

export interface InventorySummary {
  totalUnits: number;
  inStock: number;
  lowStock: number;
  outOfStock: number;
}

export interface InventoryResponse {
  items: InventoryItem[];
  summary: InventorySummary;
}

export interface StockAdjustInput {
  mode: StockAdjustMode;
  quantity: number;
  note?: string | null;
  /** Required for `set`: the stock value the admin saw. */
  expectedStock?: number;
}

export interface StockMovement {
  id: number | string;
  delta: number;
  stockAfter: number;
  reason: StockMovementReason | (string & {});
  orderNumber: string | null;
  adminEmail: string | null;
  note: string | null;
  createdAt: string;
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

export interface OrderCustomer {
  name: string;
  phone: string;
  email: string | null;
  address: string;
  city: string;
  state: string;
  pincode: string;
}

export interface OrderItem {
  productId: string;
  productName: string;
  packInfo: string;
  quantity: number;
  unitPrice: number;
  unitMrp: number;
  lineTotal: number;
}

/** Which price tier an order was charged at (two-tier pricing). */
export type PricingTier = 'bulk' | 'standard';

export interface OrderPayment {
  method: PaymentMethod;
  status: PaymentStatus;
  reference: string | null;
  paidAt: string | null;
  refundedAt: string | null;
  /** Instrument reported by Razorpay: upi | card | netbanking | wallet | emi | paylater | bank_transfer. */
  instrument: string | null;
  razorpayOrderId: string | null;
  razorpayPaymentId: string | null;
  /** Rupees refunded so far (partial refunds accumulate). */
  amountRefunded: number;
  /** Set when a payment needs human attention (e.g. a late payment whose auto-refund failed). */
  attention: string | null;
}

export interface OrderShipping {
  transportPartner: string | null;
  docketNumber: string | null;
  dispatchedAt: string | null;
  deliveredAt: string | null;
}

export interface Order {
  id: string;
  orderNumber: string;
  createdAt: string;
  updatedAt: string;
  version: number;
  customer: OrderCustomer;
  notes: string | null;
  items: OrderItem[];
  itemCount: number;
  mrpTotal: number;
  /** Goods value after tier pricing (rupees). Absent on orders placed before delivery charges — use `orderCharges()`. */
  subtotal?: number;
  /** Delivery charge (rupees, 0 = free). Absent on older orders (treat as 0). */
  deliveryFee?: number;
  /** Amount payable, including `deliveryFee`. */
  totalAmount: number;
  status: OrderStatus;
  payment: OrderPayment;
  shipping: OrderShipping;
  cancelledAt: string | null;
  cancelReason: string | null;
  /** Placed in payment test mode (charged ₹1/unit); excluded from real revenue. */
  isTest: boolean;
  /** Price tier the order was charged at; absent on orders placed before two-tier pricing. */
  pricingTier?: PricingTier;
  /** Deadline for `pending_payment` orders; stock is released after it. */
  paymentExpiresAt: string | null;
}

export interface OrderListFilters {
  status?: OrderStatus | null;
  /** Include `pending_payment` orders in an unfiltered list (excluded by default). */
  includeUnpaid?: boolean;
  paymentStatus?: PaymentStatus | null;
  q?: string | null;
  limit?: number;
}

export interface OrderListResponse {
  orders: Order[];
  nextCursor: string | null;
}

export interface OrderStatusInput {
  status: OrderStatus;
  transportPartner?: string | null;
  docketNumber?: string | null;
  expectedVersion: number;
}

export interface OrderShippingInput {
  transportPartner: string | null;
  docketNumber: string | null;
  expectedVersion: number;
}

export interface OrderCancelInput {
  reason: string;
  expectedVersion: number;
}

export interface OrderRefundInput {
  /** Rupees; `null` refunds the full remaining amount. */
  amount: number | null;
  reason: string;
  expectedVersion: number;
}

export type RefundStatus = 'pending' | 'processed' | 'failed';

export interface PaymentEvent {
  id: string;
  event: string;
  outcome: string | null;
  receivedAt: string;
}

export interface OrderRefund {
  id: string;
  razorpayRefundId: string | null;
  amount: number;
  status: RefundStatus | (string & {});
  reason: string;
  adminEmail: string | null;
  createdAt: string;
  failureReason?: string | null;
}

export interface PaymentEventsResponse {
  events: PaymentEvent[];
  refunds: OrderRefund[];
}

export interface OrderPaymentInput {
  action: PaymentAction;
  method?: PaymentMethod;
  reference?: string | null;
  expectedVersion: number;
}

// ---------------------------------------------------------------------------
// Dashboard & sync
// ---------------------------------------------------------------------------

export interface DashboardResponse {
  revenue: {
    today: number;
    last7Days: number;
    last30Days: number;
    allTime: number;
    paid: number;
    pendingCollection: number;
  };
  orders: {
    today: number;
    total: number;
    byStatus: Record<OrderStatus, number>;
  };
  /** Orders placed in payment test mode (excluded from revenue). */
  test?: { orders: number; revenue: number };
  /** `pending_payment` orders holding reserved stock. */
  unpaid?: { orders: number; amount: number };
  inventory: {
    activeProducts: number;
    totalUnits: number;
    lowStockCount: number;
    outOfStockCount: number;
    lowStock: { productId: string; name: string; stock: number; lowStockThreshold: number }[];
  };
  salesByDay: { date: string; orders: number; revenue: number }[];
  topProducts: { productId: string; name: string; unitsSold: number; revenue: number }[];
  recentOrders: Order[];
}

export interface SyncResponse {
  ordersVersion: string;
  productsVersion: string;
  pendingOrders: number;
  latestOrderNumber: string | null;
}

// ---------------------------------------------------------------------------
// Public storefront config
// ---------------------------------------------------------------------------

export interface StorefrontConfig {
  testMode: boolean;
  /** `RAZORPAY_ENABLED`; false ⇒ orders are pay-later and payments are recorded manually. */
  onlinePaymentsEnabled: boolean;
  razorpayKeyId: string | null;
  currency: string;
  paymentExpiryMinutes: number;
  storeName: string;
}
