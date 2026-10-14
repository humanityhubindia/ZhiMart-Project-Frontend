export type FilterTab = 
  | 'all'
  | 'popular'
  | 'bestselling'
  | 'newarrivals'
  | 'party'
  | 'categories'
  | 'brands';

/**
 * Public storefront product, as returned by `GET /api/v1/products` (see backend/API.md).
 * Prices are rupee numbers. Exact stock is never exposed publicly — use
 * `inStock` / `availableQuantity` (= min(stock, 1000)).
 */
export interface CrackerProduct {
  id: string;
  name: string;
  tamilName?: string | null;
  category: string; // e.g. "SINGLE SOUND CRACKERS"
  brand: string; // e.g. "Zhi-mart Fireworks"
  packInfo: string; // e.g. "25 Packet(s)", "10 Packet(s)", "1 Box(es)"
  /** Price-list (bulk) price — charged when the cart's list total reaches `PricingRules.bulkMinOrder`. */
  price: number; // e.g. 32
  originalPrice: number; // MRP, e.g. 210
  discountPercent: number; // bulk discount vs MRP, e.g. 85
  /** Standard price — charged on every line while the cart's list total is below the threshold. */
  standardPrice: number; // e.g. 63
  standardDiscountPercent: number; // e.g. 75
  image: string;
  gallery?: string[];
  description: string;
  piecesPerPack?: number | null;
  soundLevel?: string | null; // e.g. "Single Loud Blast", "Low Sound", "Silent & Visual"
  isPopular?: boolean;
  isBestSelling?: boolean;
  isNewArrival?: boolean;
  isParty?: boolean;
  inStock: boolean;
  availableQuantity: number;
}

// Legacy listing-template types (used only by the unreferenced ListingCatalog / LatestListing /
// ProductDetailView / InquiryModal / FilterModal / HeroSearch components).
export interface CartItem {
  /** Latest known snapshot of the product (refreshed whenever the catalog refetches). */
  product: CrackerProduct;
  quantity: number;
  /** Set when a catalog refetch lowered the quantity to what is available (previous quantity). */
  adjustedFrom?: number;
  /** Set when the product is archived/unknown or out of stock — blocks checkout. */
  unavailable?: boolean;
}

export interface UserProfile {
  name: string;
  phone: string;
  email?: string;
  isLoggedIn: boolean;
  isAdmin?: boolean;
}
