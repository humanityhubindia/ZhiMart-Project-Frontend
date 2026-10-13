import React, { useState, useEffect, useMemo, lazy, Suspense } from 'react';
import { Navbar } from './components/Navbar';
import { FilterTabs } from './components/FilterTabs';
import { ProductGrid } from './components/ProductGrid';
import { ProductPage } from './components/ProductPage';
import { QuickEstimatePage } from './components/QuickEstimatePage';
import { CartPage } from './components/CartPage';
import { AIAssistantPage } from './components/AIAssistantPage';
import { HeroSection } from './components/HeroSection';
import { CategoriesModal } from './components/CategoriesModal';
import { BrandsModal } from './components/BrandsModal';
import { AuthModal } from './components/AuthModal';
import { useAdminAuth } from './auth/AdminAuthContext';
import { FloatingActions } from './components/FloatingActions';
import { Footer } from './components/Footer';
import { CatalogErrorCard, ProductNotFound, ProductPageSkeleton, errorMessage } from './components/CatalogStates';
import { useCrackers } from './hooks/useCrackers';
import { findProductById, countBy } from './hooks/useProducts';
import { useCart } from './hooks/useCart';
import { CheckoutSuccess } from './hooks/useCheckout';
import { orderPageUrl, readOrderTokenFromHash } from './lib/ordersApi';
import { OrderPage } from './components/OrderPage';
import { TestModeBanner } from './components/TestModeBanner';
import { BulkUnlockBar } from './components/BulkUnlockBar';
import { CrackerProduct, FilterTab, UserProfile } from './types';

// Admin console is code-split: storefront visitors never download it.
const AdminPage = lazy(() => import('./components/AdminPage'));

/** Keys from the old localStorage-backed demo store; removed once on startup. */
const LEGACY_STORAGE_KEYS = ['zhi_mart_products', 'zhi_mart_orders', 'zhi_mart_transactions'];

type Route = {
  type: 'home' | 'product' | 'estimate' | 'cart' | 'ai-assistant' | 'admin' | 'order';
  productId?: string;
  orderNumber?: string;
  /** Open Razorpay Checkout immediately (only right after checkout, never from a reload/back). */
  autoPay?: boolean;
};

export function App() {
  // Read current route from URL query parameters (e.g. ?product=zm-02, ?page=estimate, ?page=cart,
  // ?page=ai-assistant, ?page=admin, ?page=order&order=ZM-…#t=<token>)
  const getRouteFromUrl = (): Route => {
    const params = new URLSearchParams(window.location.search);
    const qProduct = params.get('product');
    const qPage = params.get('page');
    if (qPage === 'order') return { type: 'order' as const, orderNumber: params.get('order') ?? '' };
    if (qProduct) return { type: 'product' as const, productId: qProduct };
    if (qPage === 'estimate') return { type: 'estimate' as const };
    if (qPage === 'cart') return { type: 'cart' as const };
    if (qPage === 'ai-assistant') return { type: 'ai-assistant' as const };
    if (qPage === 'admin') return { type: 'admin' as const };
    return { type: 'home' as const };
  };

  const [currentRoute, setCurrentRoute] = useState<Route>(getRouteFromUrl);

  // Email typed at checkout, kept in memory only (never stored) so the order page can say where
  // the confirmation went (the GuestOrder API intentionally doesn't return it), plus the cart's
  // price estimate so the order page can explain a server re-price.
  const [lastCheckout, setLastCheckout] = useState<{
    orderNumber: string;
    email: string;
    estimate: CheckoutSuccess['estimate'];
  } | null>(null);

  // Sync with browser back/forward buttons
  useEffect(() => {
    const handlePopState = () => {
      setCurrentRoute(getRouteFromUrl());
    };
    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  // Navigation Handlers
  const handleNavigateToProduct = (product: CrackerProduct) => {
    const newUrl = `${window.location.pathname}?product=${product.id}`;
    window.history.pushState({ productId: product.id }, '', newUrl);
    setCurrentRoute({ type: 'product', productId: product.id });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleNavigateToEstimate = () => {
    const newUrl = `${window.location.pathname}?page=estimate`;
    window.history.pushState({ page: 'estimate' }, '', newUrl);
    setCurrentRoute({ type: 'estimate' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleNavigateToCart = () => {
    const newUrl = `${window.location.pathname}?page=cart`;
    window.history.pushState({ page: 'cart' }, '', newUrl);
    setCurrentRoute({ type: 'cart' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleNavigateToAIAssistant = () => {
    const newUrl = `${window.location.pathname}?page=ai-assistant`;
    window.history.pushState({ page: 'ai-assistant' }, '', newUrl);
    setCurrentRoute({ type: 'ai-assistant' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleNavigateToAdmin = () => {
    const newUrl = `${window.location.pathname}?page=admin`;
    window.history.pushState({ page: 'admin' }, '', newUrl);
    setCurrentRoute({ type: 'admin' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  // After POST /orders: the order page (token in the URL fragment only) shows the order and,
  // when online payments are on, takes over payment.
  const handleOrderCreated = ({ order, orderToken, customer, estimate }: CheckoutSuccess) => {
    setLastCheckout({ orderNumber: order.orderNumber, email: customer.email, estimate });
    window.history.pushState({ page: 'order' }, '', orderPageUrl(order.orderNumber, orderToken));
    // Pay-later orders (online payments off) arrive as `pending_pack` — never load Razorpay for them.
    setCurrentRoute({
      type: 'order',
      orderNumber: order.orderNumber,
      autoPay: order.status === 'pending_payment',
    });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const handleBackToHome = () => {
    window.history.pushState(null, '', window.location.pathname);
    setCurrentRoute({ type: 'home' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  // One-time cleanup of the legacy demo data that used to live in localStorage.
  useEffect(() => {
    try {
      LEGACY_STORAGE_KEYS.forEach((key) => window.localStorage.removeItem(key));
    } catch {
      // Storage may be unavailable (private mode / blocked) — nothing to clean up then.
    }
  }, []);

  // Admin Session State (backed by the API; access token is held in memory only)
  const { status: adminAuthStatus, admin, logout: adminLogout } = useAdminAuth();
  const isAdminLoggedIn = adminAuthStatus === 'authenticated';

  const handleAdminLogout = () => {
    void adminLogout();
  };

  // Filters & Tabs State for Catalog
  const [activeTab, setActiveTab] = useState<FilterTab>('all');
  const [selectedCategoryFilter, setSelectedCategoryFilter] = useState<string>('all');
  const [selectedBrandFilter, setSelectedBrandFilter] = useState<string>('all');
  const [searchQuery, setSearchQuery] = useState('');

  // Live catalog from the backend (react-query), filtered client-side.
  const catalog = useCrackers({
    tab: activeTab,
    categoryFilter: selectedCategoryFilter,
    brandFilter: selectedBrandFilter,
    searchQuery,
  });
  const allProducts = catalog.allProducts;
  const categories = useMemo(() => countBy(allProducts ?? [], 'category'), [allProducts]);

  // Cart (in-memory only), reconciled against live stock on every catalog refetch.
  const cart = useCart(allProducts);

  // User State (derived from the admin session)
  const user = useMemo<UserProfile>(() => {
    if (isAdminLoggedIn && admin) {
      return {
        name: admin.displayName || admin.email,
        phone: '',
        email: admin.email,
        isLoggedIn: true,
        isAdmin: true,
      };
    }
    return {
      name: 'Customer',
      phone: '',
      isLoggedIn: false,
    };
  }, [isAdminLoggedIn, admin]);

  // Auth Modal State (Dedicated Admin Login)
  const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
  const [isCategoriesModalOpen, setIsCategoriesModalOpen] = useState(false);
  const [isBrandsModalOpen, setIsBrandsModalOpen] = useState(false);

  const totalCartCount = cart.totals.units;
  const totalCartPrice = cart.totals.total;

  // Selected Product for dedicated Product Page
  const currentProduct =
    currentRoute.type === 'product' ? findProductById(allProducts, currentRoute.productId) ?? null : null;
  const catalogError = catalog.isError && !allProducts ? errorMessage(catalog.error) : null;
  const retryCatalog = () => void catalog.refetch();

  // Grid title
  const getGridTitle = () => {
    if (selectedCategoryFilter !== 'all') return selectedCategoryFilter;
    if (selectedBrandFilter !== 'all') return `Products by ${selectedBrandFilter}`;
    if (activeTab === 'popular') return 'Popular Crackers';
    if (activeTab === 'bestselling') return 'Best Selling Crackers';
    if (activeTab === 'newarrivals') return 'New Arrivals 2025';
    if (activeTab === 'party') return 'Party & Functions Specials';
    return 'All Sivakasi Direct Products';
  };

  return (
    <div className="min-h-screen flex flex-col bg-[#F8F9FA] text-[#1E1E1E]">
      {currentRoute.type !== 'admin' && <TestModeBanner />}

      {/* If current route is NOT admin, show standard Storefront Navbar */}
      {currentRoute.type !== 'admin' && (
        <Navbar
          onOpenAuth={() => setIsAuthModalOpen(true)}
          onOpenCart={handleNavigateToCart}
          onOpenQuickEstimate={handleNavigateToEstimate}
          onOpenAIAssistant={handleNavigateToAIAssistant}
          onNavigateAdmin={handleNavigateToAdmin}
          cartCount={totalCartCount}
          cartTotal={totalCartPrice}
          cartTotals={cart.totals}
          user={user}
          onLogout={handleAdminLogout}
          onNavigateHome={handleBackToHome}
          onNavigateCatalog={() => {
            handleBackToHome();
            setTimeout(() => {
              const el = document.getElementById('products-section');
              if (el) el.scrollIntoView({ behavior: 'smooth' });
            }, 50);
          }}
          currentRoute={currentRoute.type}
        />
      )}

      {/* Main Body */}
      <main className="flex-1">
        {currentRoute.type === 'admin' ? (
          /* DEDICATED ADMIN MANAGEMENT PAGE */
          <Suspense
            fallback={
              <div className="min-h-screen bg-slate-900 flex items-center justify-center text-slate-300 text-sm font-bold">
                Loading admin console…
              </div>
            }
          >
            <AdminPage onBackToStore={handleBackToHome} />
          </Suspense>
        ) : currentRoute.type === 'ai-assistant' ? (
          /* DEDICATED AI ASSISTANT FULL PAGE */
          <AIAssistantPage
            onBack={handleBackToHome}
            onAddComboToCart={cart.addMany}
            onNavigateToCart={handleNavigateToCart}
            onNavigateToProduct={handleNavigateToProduct}
          />
        ) : currentRoute.type === 'cart' ? (
          /* DEDICATED SHOPPING CART FULL PAGE */
          <CartPage
            cartItems={cart.items}
            hasUnavailable={cart.hasUnavailable}
            maxQuantityFor={cart.maxFor}
            onUpdateQuantity={cart.updateQuantity}
            onRemoveItem={cart.removeItem}
            onClearCart={cart.clearCart}
            onBackToShopping={handleBackToHome}
            onNavigateToProduct={handleNavigateToProduct}
            onOrderCreated={handleOrderCreated}
          />
        ) : currentRoute.type === 'order' ? (
          /* GUEST ORDER STATUS + PAYMENT PAGE (?page=order&order=…#t=<token>) */
          <OrderPage
            key={currentRoute.orderNumber}
            orderNumber={currentRoute.orderNumber ?? ''}
            orderToken={readOrderTokenFromHash()}
            autoPay={currentRoute.autoPay}
            confirmationEmail={
              lastCheckout && lastCheckout.orderNumber === currentRoute.orderNumber ? lastCheckout.email : null
            }
            clientEstimate={
              lastCheckout && lastCheckout.orderNumber === currentRoute.orderNumber ? lastCheckout.estimate : null
            }
            onShopAgain={handleBackToHome}
          />
        ) : currentRoute.type === 'estimate' ? (
          /* DEDICATED QUICK ESTIMATE FULL PAGE */
          <QuickEstimatePage
            onBack={handleBackToHome}
            onAddEstimateToCart={(items) => {
              cart.addMany(items);
              handleNavigateToCart();
            }}
          />
        ) : currentRoute.type === 'product' ? (
          /* DEDICATED PRODUCT DETAIL FULL PAGE (deep link: ?product=<id>) */
          catalog.isLoading ? (
            <ProductPageSkeleton />
          ) : catalogError ? (
            <div className="max-w-3xl mx-auto px-4 sm:px-6 py-12">
              <CatalogErrorCard
                title="We couldn't load this product"
                message={catalogError}
                onRetry={retryCatalog}
                isRetrying={catalog.isFetching}
              />
            </div>
          ) : currentProduct ? (
            <ProductPage
              key={currentProduct.id}
              product={currentProduct}
              quantityInCart={cart.quantityOf(currentProduct.id)}
              onBack={handleBackToHome}
              onAddToCart={(product) => cart.addToCart(product)}
              onUpdateQuantity={(product, newQty) => cart.updateQuantity(product.id, newQty)}
              onGoToCart={handleNavigateToCart}
            />
          ) : (
            <ProductNotFound productId={currentRoute.productId} onBack={handleBackToHome} />
          )
        ) : (
          /* MAIN HOME CATALOG VIEW */
          <>
            {/* Hero Section Highlighting Direct Sivakasi Dispatch & Free Delivery */}
            <HeroSection
              onExploreProducts={() => {
                const el = document.getElementById('products-section');
                if (el) el.scrollIntoView({ behavior: 'smooth' });
              }}
              onOpenQuickEstimate={handleNavigateToEstimate}
              onOpenAIAssistant={handleNavigateToAIAssistant}
            />

            {/* Top Filter Navigation Bar - All Products & Categories */}
            <FilterTabs
              selectedCategory={selectedCategoryFilter}
              onSelectCategory={(category) => {
                setSelectedCategoryFilter(category);
                setActiveTab('all');
                setSelectedBrandFilter('all');
              }}
              categories={categories}
            />

            {/* Product Grid with live dynamic products */}
            <ProductGrid
              title={getGridTitle()}
              products={catalog.products}
              cartItems={cart.items}
              onAddToCart={(product) => cart.addToCart(product)}
              onUpdateQuantity={(product, newQty) => cart.updateQuantity(product.id, newQty)}
              onOpenDetail={handleNavigateToProduct}
              isLoading={catalog.isLoading}
              error={catalogError}
              onRetry={retryCatalog}
              isRetrying={catalog.isFetching}
            />
          </>
        )}
      </main>

      {/* Footer (shown on storefront pages) */}
      {currentRoute.type !== 'admin' && <Footer />}

      {/* "₹X more for 85% OFF" hint while browsing (catalog + product pages only). */}
      {currentRoute.type !== 'admin' && (
        <BulkUnlockBar
          totals={cart.totals}
          onOpenCart={handleNavigateToCart}
          suppressed={currentRoute.type !== 'home' && currentRoute.type !== 'product'}
        />
      )}

      {/* Floating Actions (WhatsApp + Scroll to Top) */}
      {currentRoute.type !== 'admin' && <FloatingActions />}

      {/* Categories & Brands Modals */}
      <CategoriesModal
        isOpen={isCategoriesModalOpen}
        onClose={() => setIsCategoriesModalOpen(false)}
        onSelectCategory={(cat) => {
          setSelectedCategoryFilter(cat);
          setActiveTab('all');
          handleBackToHome();
        }}
      />

      <BrandsModal
        isOpen={isBrandsModalOpen}
        onClose={() => setIsBrandsModalOpen(false)}
        onSelectBrand={(brand) => {
          setSelectedBrandFilter(brand);
          setActiveTab('all');
          handleBackToHome();
        }}
      />

      {/* Dedicated Admin Login Modal */}
      <AuthModal
        isOpen={isAuthModalOpen}
        onClose={() => setIsAuthModalOpen(false)}
        onNavigateToAdmin={handleNavigateToAdmin}
      />
    </div>
  );
}

export default App;
