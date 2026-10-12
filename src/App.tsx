import React, { useState, useEffect, useMemo } from 'react';
import { Navbar } from './components/Navbar';
import { FilterTabs } from './components/FilterTabs';
import { ProductGrid } from './components/ProductGrid';
import { ProductPage } from './components/ProductPage';
import { QuickEstimatePage } from './components/QuickEstimatePage';
import { CartPage } from './components/CartPage';
import { AIAssistantPage } from './components/AIAssistantPage';
import { AdminPage } from './components/AdminPage';
import { HeroSection } from './components/HeroSection';
import { CategoriesModal } from './components/CategoriesModal';
import { BrandsModal } from './components/BrandsModal';
import { AuthModal } from './components/AuthModal';
import { FloatingActions } from './components/FloatingActions';
import { Footer } from './components/Footer';
import { ZHI_MART_PRODUCTS } from './data/crackers';
import { INITIAL_ADMIN_ORDERS, INITIAL_RAZORPAY_TRANSACTIONS } from './data/adminMockData';
import {
  CrackerProduct,
  CartItem,
  FilterTab,
  UserProfile,
  AdminOrder,
  RazorpayTransaction,
  OrderStatus,
} from './types';

export function App() {
  // Read current route from URL query parameters (e.g. ?product=zm-02, ?page=estimate, ?page=cart, ?page=ai-assistant, ?page=admin)
  const getRouteFromUrl = () => {
    const params = new URLSearchParams(window.location.search);
    const qProduct = params.get('product');
    const qPage = params.get('page');
    if (qProduct) return { type: 'product' as const, productId: qProduct };
    if (qPage === 'estimate') return { type: 'estimate' as const };
    if (qPage === 'cart') return { type: 'cart' as const };
    if (qPage === 'ai-assistant') return { type: 'ai-assistant' as const };
    if (qPage === 'admin') return { type: 'admin' as const };
    return { type: 'home' as const };
  };

  const [currentRoute, setCurrentRoute] = useState<{
    type: 'home' | 'product' | 'estimate' | 'cart' | 'ai-assistant' | 'admin';
    productId?: string;
  }>(getRouteFromUrl);

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

  const handleBackToHome = () => {
    window.history.pushState(null, '', window.location.pathname);
    setCurrentRoute({ type: 'home' });
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  // Dynamic Product Store with persistence
  const [productsList, setProductsList] = useState<CrackerProduct[]>(() => {
    const saved = localStorage.getItem('zhi_mart_products');
    if (saved) {
      try {
        return JSON.parse(saved);
      } catch (e) {
        // fallback
      }
    }
    return ZHI_MART_PRODUCTS.map((p, idx) => ({
      ...p,
      stock: p.stock ?? (120 - (idx % 6) * 15),
      bayLocation:
        p.bayLocation ??
        (idx % 3 === 0
          ? 'Bay B-1 (Sound Bunker)'
          : idx % 3 === 1
          ? 'Bay A-2 (Aerial Shells)'
          : 'Bay C-1 (Sparklers Hall)'),
    }));
  });

  useEffect(() => {
    localStorage.setItem('zhi_mart_products', JSON.stringify(productsList));
  }, [productsList]);

  // Dynamic Orders Store with persistence
  const [ordersList, setOrdersList] = useState<AdminOrder[]>(() => {
    const saved = localStorage.getItem('zhi_mart_orders');
    if (saved) {
      try {
        return JSON.parse(saved);
      } catch (e) {
        // fallback
      }
    }
    return INITIAL_ADMIN_ORDERS;
  });

  useEffect(() => {
    localStorage.setItem('zhi_mart_orders', JSON.stringify(ordersList));
  }, [ordersList]);

  // Dynamic Razorpay Transactions Store with persistence
  const [transactionsList, setTransactionsList] = useState<RazorpayTransaction[]>(() => {
    const saved = localStorage.getItem('zhi_mart_transactions');
    if (saved) {
      try {
        return JSON.parse(saved);
      } catch (e) {
        // fallback
      }
    }
    return INITIAL_RAZORPAY_TRANSACTIONS;
  });

  useEffect(() => {
    localStorage.setItem('zhi_mart_transactions', JSON.stringify(transactionsList));
  }, [transactionsList]);

  // Admin Session State
  const [isAdminLoggedIn, setIsAdminLoggedIn] = useState<boolean>(() => {
    return localStorage.getItem('zhi_mart_admin') === 'true';
  });

  const handleAdminLogin = () => {
    setIsAdminLoggedIn(true);
    localStorage.setItem('zhi_mart_admin', 'true');
    setUser({
      name: 'Admin (Operations)',
      phone: '+91 98400 12345',
      email: 'admin@zhimart.com',
      isLoggedIn: true,
      isAdmin: true,
    });
  };

  const handleAdminLogout = () => {
    setIsAdminLoggedIn(false);
    localStorage.removeItem('zhi_mart_admin');
    setUser({
      name: '',
      phone: '',
      isLoggedIn: false,
      isAdmin: false,
    });
  };

  // Product CRUD Handlers
  const handleAddProduct = (newProduct: CrackerProduct) => {
    setProductsList((prev) => [newProduct, ...prev]);
  };

  const handleUpdateProduct = (updatedProduct: CrackerProduct) => {
    setProductsList((prev) =>
      prev.map((p) => (p.id === updatedProduct.id ? updatedProduct : p))
    );
  };

  const handleDeleteProduct = (productId: string) => {
    setProductsList((prev) => prev.filter((p) => p.id !== productId));
  };

  const handleUpdateStock = (productId: string, newStock: number) => {
    setProductsList((prev) =>
      prev.map((p) => (p.id === productId ? { ...p, stock: newStock } : p))
    );
  };

  // Order & Razorpay Handlers
  const handleUpdateOrderStatus = (
    orderId: string,
    status: OrderStatus,
    transportPartner?: string,
    docketNumber?: string
  ) => {
    setOrdersList((prev) =>
      prev.map((o) => {
        if (o.id === orderId) {
          return {
            ...o,
            status,
            transportPartner: transportPartner ?? o.transportPartner,
            docketNumber: docketNumber ?? o.docketNumber,
            dispatchedAt:
              status === 'dispatched'
                ? `Today, ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })} (Sivakasi Yard)`
                : o.dispatchedAt,
          };
        }
        return o;
      })
    );
  };

  const handleRefundTransaction = (transactionId: string) => {
    setTransactionsList((prev) =>
      prev.map((t) => (t.id === transactionId ? { ...t, status: 'refunded' } : t))
    );
  };

  // Filters & Tabs State for Catalog
  const [activeTab, setActiveTab] = useState<FilterTab>('all');
  const [selectedCategoryFilter, setSelectedCategoryFilter] = useState<string>('all');
  const [selectedBrandFilter, setSelectedBrandFilter] = useState<string>('all');
  const [searchQuery, setSearchQuery] = useState('');

  // Live Catalog Filtering based on dynamic productsList
  const filteredCatalogProducts = useMemo(() => {
    let result = [...productsList];

    if (activeTab === 'popular') {
      result = result.filter((p) => p.isPopular);
    } else if (activeTab === 'bestselling') {
      result = result.filter((p) => p.isBestSelling);
    } else if (activeTab === 'newarrivals') {
      result = result.filter((p) => p.isNewArrival);
    } else if (activeTab === 'party') {
      result = result.filter((p) => p.isParty);
    }

    if (selectedCategoryFilter !== 'all') {
      result = result.filter((p) =>
        p.category.toLowerCase().includes(selectedCategoryFilter.toLowerCase())
      );
    }

    if (selectedBrandFilter !== 'all') {
      result = result.filter((p) =>
        p.brand.toLowerCase().includes(selectedBrandFilter.toLowerCase())
      );
    }

    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase().trim();
      result = result.filter(
        (p) =>
          p.name.toLowerCase().includes(q) ||
          (p.tamilName && p.tamilName.toLowerCase().includes(q)) ||
          p.category.toLowerCase().includes(q) ||
          p.brand.toLowerCase().includes(q)
      );
    }

    return result;
  }, [productsList, activeTab, selectedCategoryFilter, selectedBrandFilter, searchQuery]);

  // Cart State
  const [cartItems, setCartItems] = useState<CartItem[]>([
    {
      product: productsList[0] || ZHI_MART_PRODUCTS[0],
      quantity: 1,
    },
  ]);

  // User State
  const [user, setUser] = useState<UserProfile>(() => {
    const adminSaved = localStorage.getItem('zhi_mart_admin') === 'true';
    if (adminSaved) {
      return {
        name: 'Admin (Operations)',
        phone: '+91 98400 12345',
        email: 'admin@zhimart.com',
        isLoggedIn: true,
        isAdmin: true,
      };
    }
    return {
      name: 'Customer',
      phone: '',
      isLoggedIn: false,
    };
  });

  // Auth Modal State (Dedicated Admin Login)
  const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
  const [isCategoriesModalOpen, setIsCategoriesModalOpen] = useState(false);
  const [isBrandsModalOpen, setIsBrandsModalOpen] = useState(false);

  // Cart Handlers
  const handleAddToCart = (product: CrackerProduct) => {
    setCartItems((prev) => {
      const existing = prev.find((item) => item.product.id === product.id);
      if (existing) {
        return prev.map((item) =>
          item.product.id === product.id
            ? { ...item, quantity: item.quantity + 1 }
            : item
        );
      }
      return [...prev, { product, quantity: 1 }];
    });
  };

  const handleUpdateQuantity = (productId: string, newQty: number) => {
    if (newQty <= 0) {
      handleRemoveItem(productId);
      return;
    }
    setCartItems((prev) =>
      prev.map((item) =>
        item.product.id === productId ? { ...item, quantity: newQty } : item
      )
    );
  };

  const handleRemoveItem = (productId: string) => {
    setCartItems((prev) => prev.filter((item) => item.product.id !== productId));
  };

  const handleAddMultipleToCart = (itemsToAdd: { product: CrackerProduct; quantity: number }[]) => {
    setCartItems((prev) => {
      const updated = [...prev];
      itemsToAdd.forEach(({ product, quantity }) => {
        const idx = updated.findIndex((i) => i.product.id === product.id);
        if (idx >= 0) {
          updated[idx].quantity += quantity;
        } else {
          updated.push({ product, quantity });
        }
      });
      return updated;
    });
  };

  const totalCartCount = cartItems.reduce((acc, item) => acc + item.quantity, 0);
  const totalCartPrice = cartItems.reduce(
    (acc, item) => acc + item.product.price * item.quantity,
    0
  );

  // Selected Product for dedicated Product Page
  const currentProduct =
    currentRoute.type === 'product' && currentRoute.productId
      ? productsList.find((p) => p.id === currentRoute.productId) || null
      : null;

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
          <AdminPage
            products={productsList}
            orders={ordersList}
            transactions={transactionsList}
            onAddProduct={handleAddProduct}
            onUpdateProduct={handleUpdateProduct}
            onDeleteProduct={handleDeleteProduct}
            onUpdateOrderStatus={handleUpdateOrderStatus}
            onRefundTransaction={handleRefundTransaction}
            onUpdateStock={handleUpdateStock}
            onBackToStore={handleBackToHome}
            isAdminLoggedIn={isAdminLoggedIn}
            onLogin={handleAdminLogin}
            onLogout={handleAdminLogout}
          />
        ) : currentRoute.type === 'ai-assistant' ? (
          /* DEDICATED AI ASSISTANT FULL PAGE */
          <AIAssistantPage
            onBack={handleBackToHome}
            onAddComboToCart={(items) => {
              handleAddMultipleToCart(items);
            }}
            onNavigateToCart={handleNavigateToCart}
            onNavigateToProduct={handleNavigateToProduct}
          />
        ) : currentRoute.type === 'cart' ? (
          /* DEDICATED SHOPPING CART FULL PAGE */
          <CartPage
            cartItems={cartItems}
            onUpdateQuantity={handleUpdateQuantity}
            onRemoveItem={handleRemoveItem}
            onClearCart={() => setCartItems([])}
            onBackToShopping={handleBackToHome}
            onNavigateToProduct={handleNavigateToProduct}
          />
        ) : currentRoute.type === 'estimate' ? (
          /* DEDICATED QUICK ESTIMATE FULL PAGE */
          <QuickEstimatePage
            onBack={handleBackToHome}
            onAddEstimateToCart={(items) => {
              handleAddMultipleToCart(items);
              handleNavigateToCart();
            }}
          />
        ) : currentProduct ? (
          /* DEDICATED PRODUCT DETAIL FULL PAGE */
          <ProductPage
            product={currentProduct}
            quantityInCart={
              cartItems.find((i) => i.product.id === currentProduct.id)?.quantity || 0
            }
            onBack={handleBackToHome}
            onAddToCart={handleAddToCart}
            onUpdateQuantity={(product, newQty) =>
              handleUpdateQuantity(product.id, newQty)
            }
            onGoToCart={handleNavigateToCart}
          />
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

            {/* Top Filter Navigation Bar */}
            <FilterTabs
              activeTab={activeTab}
              onSelectTab={(tab) => {
                setActiveTab(tab);
                setSelectedCategoryFilter('all');
                setSelectedBrandFilter('all');
              }}
              onOpenCategoriesModal={() => setIsCategoriesModalOpen(true)}
              onOpenBrandsModal={() => setIsBrandsModalOpen(true)}
            />

            {/* Product Grid with live dynamic products */}
            <ProductGrid
              title={getGridTitle()}
              products={filteredCatalogProducts}
              cartItems={cartItems}
              onAddToCart={handleAddToCart}
              onUpdateQuantity={(product, newQty) =>
                handleUpdateQuantity(product.id, newQty)
              }
              onOpenDetail={handleNavigateToProduct}
            />
          </>
        )}
      </main>

      {/* Footer (shown on storefront pages) */}
      {currentRoute.type !== 'admin' && <Footer />}

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
        onLoginSuccess={(loggedInUser) => {
          setUser(loggedInUser);
          setIsAdminLoggedIn(true);
        }}
        onNavigateToAdmin={handleNavigateToAdmin}
      />
    </div>
  );
}

export default App;
