export type FilterTab = 
  | 'all'
  | 'popular'
  | 'bestselling'
  | 'newarrivals'
  | 'party'
  | 'categories'
  | 'brands';

export interface CrackerProduct {
  id: string;
  name: string;
  category: string; // e.g. "SINGLE SOUND CRACKERS"
  brand: string; // e.g. "Zhi-mart Fireworks"
  packInfo: string; // e.g. "25 Packet(s)", "10 Packet(s)", "1 Box(es)"
  price: number; // e.g. 205
  originalPrice: number; // e.g. 373
  discountPercent: number; // e.g. 45
  image: string;
  gallery?: string[];
  description: string;
  piecesPerPack?: number;
  soundLevel?: 'Low Sound' | 'Single Loud Blast' | 'Repetitive Blast' | 'Silent & Visual';
  isPopular?: boolean;
  isBestSelling?: boolean;
  isNewArrival?: boolean;
  isParty?: boolean;
  stock?: number;
  bayLocation?: string;
  tamilName?: string;
}

export type CrackerCategory = 
  | 'all'
  | 'aerial'
  | 'spinners'
  | 'fountains'
  | 'sparklers'
  | 'hampers'
  | 'bulk';

export interface CrackerItem {
  id: string;
  title: string;
  subtitle: string;
  category: CrackerCategory;
  categoryLabel: string;
  priceMin: number;
  priceMax: number;
  currency: string;
  image: string;
  gallery: string[];
  artisan: string;
  location: string;
  specs: {
    spec1: string;
    spec2: string;
    spec3: string;
    spec4: string;
  };
  description: string;
  extendedDescription?: string;
  features: string[];
  pricingTiers: {
    title: string;
    period: string;
    price: number;
  }[];
  isPopular?: boolean;
  isLatest?: boolean;
}

export interface CartItem {
  product: CrackerProduct;
  quantity: number;
}

export interface UserProfile {
  name: string;
  phone: string;
  email?: string;
  isLoggedIn: boolean;
  isAdmin?: boolean;
}

export type OrderStatus =
  | 'pending_pack'
  | 'dispatched'
  | 'in_transit'
  | 'out_for_delivery'
  | 'delivered';

export interface AdminOrder {
  id: string;
  date: string;
  customerName: string;
  customerPhone: string;
  customerCity: string;
  customerAddress: string;
  items: {
    productId: string;
    productName: string;
    quantity: number;
    price: number;
  }[];
  totalAmount: number;
  paymentMethod: 'Razorpay UPI' | 'Razorpay Card' | 'Razorpay NetBanking' | 'COD Advance';
  paymentId?: string;
  status: OrderStatus;
  transportPartner?: string;
  docketNumber?: string;
  dispatchedAt?: string;
}

export type PaymentStatus = 'captured' | 'authorized' | 'refunded' | 'failed';

export interface RazorpayTransaction {
  id: string;
  orderId: string;
  customerName: string;
  customerEmail: string;
  customerPhone: string;
  amount: number;
  method: 'UPI' | 'Card' | 'NetBanking' | 'Wallet';
  methodDetail: string;
  status: PaymentStatus;
  createdAt: string;
  rrn: string;
  fee: number;
  tax: number;
}
