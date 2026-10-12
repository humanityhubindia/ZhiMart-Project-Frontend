import React, { useState, useMemo } from 'react';
import {
  LayoutDashboard,
  Package,
  ShoppingBag,
  CreditCard,
  Warehouse,
  Plus,
  Search,
  Filter,
  Edit3,
  Trash2,
  CheckCircle2,
  Clock,
  AlertTriangle,
  Truck,
  ArrowUpRight,
  TrendingUp,
  RefreshCw,
  LogOut,
  Lock,
  Mail,
  Eye,
  EyeOff,
  ExternalLink,
  ChevronRight,
  ShieldCheck,
  Check,
  X,
  FileText,
  Phone,
  MapPin,
  MessageCircle,
  Building2,
  AlertCircle,
  ArrowLeft,
  DollarSign,
  Printer,
  Download,
} from 'lucide-react';
import {
  CrackerProduct,
  AdminOrder,
  OrderStatus,
  RazorpayTransaction,
  PaymentStatus,
} from '../types';

interface AdminPageProps {
  products: CrackerProduct[];
  orders: AdminOrder[];
  transactions: RazorpayTransaction[];
  onAddProduct: (product: CrackerProduct) => void;
  onUpdateProduct: (product: CrackerProduct) => void;
  onDeleteProduct: (productId: string) => void;
  onUpdateOrderStatus: (
    orderId: string,
    status: OrderStatus,
    transportPartner?: string,
    docketNumber?: string
  ) => void;
  onRefundTransaction: (transactionId: string) => void;
  onUpdateStock: (productId: string, newStock: number) => void;
  onBackToStore: () => void;
  isAdminLoggedIn: boolean;
  onLogin: () => void;
  onLogout: () => void;
}

type AdminTab = 'analytics' | 'orders' | 'products' | 'razorpay' | 'inventory';

const PRESET_PRODUCT_IMAGES = [
  { label: 'Lakshmi Single Blast Box', url: '/assets/products/lakshmi_box.jpg' },
  { label: 'Flower Pots Big Cones', url: '/assets/products/flower_pots.jpg' },
  { label: 'Ground Chakras Wheel', url: '/assets/products/chakras.jpg' },
  { label: 'Aerial Multi-Shot Cake', url: '/assets/products/aerial_shots.jpg' },
  { label: 'Festival Family Hamper', url: '/assets/products/hamper_box.jpg' },
];

function numberToWords(num: number): string {
  const a = [
    '', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine',
    'Ten', 'Eleven', 'Twelve', 'Thirteen', 'Fourteen', 'Fifteen', 'Sixteen',
    'Seventeen', 'Eighteen', 'Nineteen'
  ];
  const b = ['', '', 'Twenty', 'Thirty', 'Forty', 'Fifty', 'Sixty', 'Seventy', 'Eighty', 'Ninety'];

  if (num === 0) return 'Zero';
  function inWords(n: number): string {
    if (n < 20) return a[n];
    if (n < 100) return b[Math.floor(n / 10)] + (n % 10 !== 0 ? ' ' + a[n % 10] : '');
    if (n < 1000) return a[Math.floor(n / 100)] + ' Hundred' + (n % 100 !== 0 ? ' ' + inWords(n % 100) : '');
    if (n < 100000) return inWords(Math.floor(n / 1000)) + ' Thousand' + (n % 1000 !== 0 ? ' ' + inWords(n % 1000) : '');
    if (n < 10000000) return inWords(Math.floor(n / 100000)) + ' Lakh' + (n % 100000 !== 0 ? ' ' + inWords(n % 100000) : '');
    return inWords(Math.floor(n / 10000000)) + ' Crore' + (n % 10000000 !== 0 ? ' ' + inWords(n % 10000000) : '');
  }
  return inWords(Math.floor(num)) + ' Rupees Only';
}

export const AdminPage: React.FC<AdminPageProps> = ({
  products,
  orders,
  transactions,
  onAddProduct,
  onUpdateProduct,
  onDeleteProduct,
  onUpdateOrderStatus,
  onRefundTransaction,
  onUpdateStock,
  onBackToStore,
  isAdminLoggedIn,
  onLogin,
  onLogout,
}) => {
  // Current Active Tab
  const [activeTab, setActiveTab] = useState<AdminTab>('analytics');

  // Login form state
  const [loginEmail, setLoginEmail] = useState('admin@zhimart.com');
  const [loginPassword, setLoginPassword] = useState('admin123');
  const [loginError, setLoginError] = useState('');
  const [showPassword, setShowPassword] = useState(false);

  // Orders Tab State
  const [orderFilter, setOrderFilter] = useState<'all' | OrderStatus>('all');
  const [orderSearch, setOrderSearch] = useState('');
  const [selectedOrderForDocket, setSelectedOrderForDocket] = useState<AdminOrder | null>(null);
  const [selectedOrderForInvoice, setSelectedOrderForInvoice] = useState<AdminOrder | null>(null);
  const [editTransportPartner, setEditTransportPartner] = useState('');
  const [editDocketNo, setEditDocketNo] = useState('');

  // Products Tab (CRUD) State
  const [productSearch, setProductSearch] = useState('');
  const [productCategoryFilter, setProductCategoryFilter] = useState('all');
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [editingProduct, setEditingProduct] = useState<CrackerProduct | null>(null);
  const [deletingProductId, setDeletingProductId] = useState<string | null>(null);

  // New / Edit Product Form State
  const [formName, setFormName] = useState('');
  const [formTamilName, setFormTamilName] = useState('');
  const [formCategory, setFormCategory] = useState('SINGLE SOUND CRACKERS');
  const [formBrand, setFormBrand] = useState('Zhi-mart Fireworks');
  const [formPackInfo, setFormPackInfo] = useState('10 Packet(s)');
  const [formOriginalPrice, setFormOriginalPrice] = useState('300');
  const [formPrice, setFormPrice] = useState('165');
  const [formSoundLevel, setFormSoundLevel] = useState<CrackerProduct['soundLevel']>('Single Loud Blast');
  const [formStock, setFormStock] = useState('100');
  const [formBayLocation, setFormBayLocation] = useState('Bay B-1 (Sound Bunker)');
  const [formImage, setFormImage] = useState('/assets/products/lakshmi_box.jpg');
  const [formDescription, setFormDescription] = useState('');

  // Razorpay Tab State
  const [razorpayFilter, setRazorpayFilter] = useState<'all' | PaymentStatus>('all');
  const [razorpaySearch, setRazorpaySearch] = useState('');
  const [viewingReceipt, setViewingReceipt] = useState<RazorpayTransaction | null>(null);

  // Inventory Tab State
  const [inventorySearch, setInventorySearch] = useState('');
  const [stockEditMap, setStockEditMap] = useState<Record<string, number>>({});
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  // 1. Handle Admin Login
  const handleLoginSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (loginEmail === 'admin@zhimart.com' && loginPassword === 'admin123') {
      setLoginError('');
      onLogin();
    } else {
      setLoginError('Invalid credentials. Use admin@zhimart.com / admin123');
    }
  };

  const handleQuickDemoLogin = () => {
    setLoginEmail('admin@zhimart.com');
    setLoginPassword('admin123');
    setLoginError('');
    onLogin();
  };

  // 2. Open Add Product Modal
  const handleOpenAddModal = () => {
    setFormName('');
    setFormTamilName('');
    setFormCategory('SINGLE SOUND CRACKERS');
    setFormBrand('Zhi-mart Fireworks');
    setFormPackInfo('10 Packet(s)');
    setFormOriginalPrice('400');
    setFormPrice('220');
    setFormSoundLevel('Single Loud Blast');
    setFormStock('120');
    setFormBayLocation('Bay B-2 (Sound Bunker)');
    setFormImage('/assets/products/lakshmi_box.jpg');
    setFormDescription('Manufactured in Sivakasi with certified green pyrotechnic formula.');
    setIsAddModalOpen(true);
  };

  // 3. Open Edit Product Modal
  const handleOpenEditModal = (p: CrackerProduct) => {
    setEditingProduct(p);
    setFormName(p.name);
    setFormTamilName(p.tamilName || '');
    setFormCategory(p.category);
    setFormBrand(p.brand);
    setFormPackInfo(p.packInfo);
    setFormOriginalPrice(p.originalPrice.toString());
    setFormPrice(p.price.toString());
    setFormSoundLevel(p.soundLevel || 'Single Loud Blast');
    setFormStock((p.stock ?? 100).toString());
    setFormBayLocation(p.bayLocation || 'Bay B-1 (Sound Bunker)');
    setFormImage(p.image);
    setFormDescription(p.description);
  };

  // 4. Save Product (Add or Edit)
  const handleSaveProduct = (e: React.FormEvent) => {
    e.preventDefault();
    const origPriceNum = parseFloat(formOriginalPrice) || 100;
    const priceNum = parseFloat(formPrice) || Math.round(origPriceNum * 0.55);
    const stockNum = parseInt(formStock, 10) || 50;
    const discount = Math.round(((origPriceNum - priceNum) / origPriceNum) * 100);

    if (editingProduct) {
      const updated: CrackerProduct = {
        ...editingProduct,
        name: formName,
        tamilName: formTamilName || undefined,
        category: formCategory,
        brand: formBrand,
        packInfo: formPackInfo,
        originalPrice: origPriceNum,
        price: priceNum,
        discountPercent: discount,
        soundLevel: formSoundLevel,
        stock: stockNum,
        bayLocation: formBayLocation,
        image: formImage,
        description: formDescription,
      };
      onUpdateProduct(updated);
      setEditingProduct(null);
      showToast(`Updated product: ${formName}`);
    } else {
      const newProduct: CrackerProduct = {
        id: `zm-${Date.now().toString().slice(-4)}`,
        name: formName,
        tamilName: formTamilName || undefined,
        category: formCategory,
        brand: formBrand,
        packInfo: formPackInfo,
        originalPrice: origPriceNum,
        price: priceNum,
        discountPercent: discount,
        soundLevel: formSoundLevel,
        stock: stockNum,
        bayLocation: formBayLocation,
        image: formImage,
        description: formDescription,
        isPopular: true,
      };
      onAddProduct(newProduct);
      setIsAddModalOpen(false);
      showToast(`Added new product: ${formName}`);
    }
  };

  // 5. Open Docket Editor
  const handleOpenDocketModal = (order: AdminOrder) => {
    setSelectedOrderForDocket(order);
    setEditTransportPartner(order.transportPartner || 'VRL Logistics');
    setEditDocketNo(order.docketNumber || `LR-VRL-${Math.floor(100000 + Math.random() * 900000)}`);
  };

  const handleSaveDocket = (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedOrderForDocket) return;
    onUpdateOrderStatus(
      selectedOrderForDocket.id,
      selectedOrderForDocket.status,
      editTransportPartner,
      editDocketNo
    );
    setSelectedOrderForDocket(null);
    showToast(`Updated tracking docket for ${selectedOrderForDocket.id}`);
  };

  // 6. WhatsApp Dispatch Notification
  const handleSendWhatsAppUpdate = (order: AdminOrder) => {
    const text =
      `*ZHI-MART FACTORY DISPATCH UPDATE*\n` +
      `---------------------------------\n` +
      `Order: *${order.id}*\n` +
      `Customer: *${order.customerName}*\n` +
      `Status: *${order.status.replace('_', ' ').toUpperCase()}*\n` +
      `Carrier: *${order.transportPartner || 'VRL Logistics'}*\n` +
      `Docket / LR No: *${order.docketNumber || 'Generating...'}*\n` +
      `Amount: ₹${order.totalAmount}\n` +
      `---------------------------------\n` +
      `Direct dispatch from Sivakasi factory. Track at transport parcel hub. Thank you!`;
    const cleanPhone = order.customerPhone.replace(/[^0-9]/g, '');
    window.open(`https://wa.me/${cleanPhone}?text=${encodeURIComponent(text)}`, '_blank');
  };

  // Calculation Metrics for Analytics Tab
  const totalRevenue = useMemo(() => {
    return transactions
      .filter((t) => t.status === 'captured')
      .reduce((acc, t) => acc + t.amount, 0);
  }, [transactions]);

  const totalOrdersCount = orders.length;
  const pendingOrdersCount = orders.filter((o) => o.status === 'pending_pack').length;
  const dispatchedOrdersCount = orders.filter((o) => o.status === 'dispatched' || o.status === 'in_transit').length;
  const deliveredOrdersCount = orders.filter((o) => o.status === 'delivered').length;

  const lowStockProducts = useMemo(() => {
    return products.filter((p) => (p.stock ?? 100) < 30);
  }, [products]);

  // Categories list for filtering
  const allCategories = useMemo(() => {
    const set = new Set<string>();
    products.forEach((p) => set.add(p.category));
    return Array.from(set);
  }, [products]);

  // Filtered Orders
  const filteredOrders = useMemo(() => {
    return orders.filter((o) => {
      if (orderFilter !== 'all' && o.status !== orderFilter) return false;
      if (orderSearch.trim()) {
        const q = orderSearch.toLowerCase();
        return (
          o.id.toLowerCase().includes(q) ||
          o.customerName.toLowerCase().includes(q) ||
          o.customerPhone.includes(q) ||
          o.customerCity.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }, [orders, orderFilter, orderSearch]);

  // Filtered Products
  const filteredProducts = useMemo(() => {
    return products.filter((p) => {
      if (productCategoryFilter !== 'all' && p.category !== productCategoryFilter) return false;
      if (productSearch.trim()) {
        const q = productSearch.toLowerCase();
        return (
          p.name.toLowerCase().includes(q) ||
          (p.tamilName && p.tamilName.toLowerCase().includes(q)) ||
          p.category.toLowerCase().includes(q) ||
          p.id.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }, [products, productCategoryFilter, productSearch]);

  // Filtered Transactions
  const filteredTransactions = useMemo(() => {
    return transactions.filter((t) => {
      if (razorpayFilter !== 'all' && t.status !== razorpayFilter) return false;
      if (razorpaySearch.trim()) {
        const q = razorpaySearch.toLowerCase();
        return (
          t.id.toLowerCase().includes(q) ||
          t.orderId.toLowerCase().includes(q) ||
          t.customerName.toLowerCase().includes(q) ||
          t.customerPhone.includes(q) ||
          t.methodDetail.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }, [transactions, razorpayFilter, razorpaySearch]);

  // If NOT logged in, show dedicated Admin Login View
  if (!isAdminLoggedIn) {
    return (
      <div className="min-h-screen bg-slate-900 flex items-center justify-center p-4 sm:p-6 text-slate-100">
        <div className="w-full max-w-md bg-white text-slate-900 rounded-3xl p-8 sm:p-10 shadow-2xl border border-slate-200 animate-in fade-in zoom-in-95 duration-200">
          {/* Top Brand & Security Emblem */}
          <div className="flex flex-col items-center text-center mb-8">
            <div className="w-14 h-14 rounded-2xl bg-gradient-to-tr from-slate-950 to-slate-800 p-1 shadow-md mb-3 flex items-center justify-center">
              <span
                className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-2xl select-none"
                style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
              >
                ழி
              </span>
            </div>
            <h1 className="text-2xl font-black tracking-tight text-slate-950 font-heading">
              ZHI-MART ADMIN
            </h1>
            <p className="text-xs font-bold text-amber-600 uppercase tracking-widest mt-1">
              Sivakasi Factory Operations Console
            </p>
            <p className="text-xs text-slate-500 mt-2 max-w-xs">
              Restricted management portal for Sivakasi live dispatch, catalog CRUD, orders, and Razorpay settlements.
            </p>
          </div>

          {loginError && (
            <div className="mb-6 p-3.5 rounded-xl bg-rose-50 border border-rose-200 text-rose-700 text-xs font-bold flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0" />
              <span>{loginError}</span>
            </div>
          )}

          {/* Form */}
          <form onSubmit={handleLoginSubmit} className="space-y-4">
            <div>
              <label className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5">
                Admin Email ID
              </label>
              <div className="relative">
                <Mail className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="email"
                  value={loginEmail}
                  onChange={(e) => setLoginEmail(e.target.value)}
                  placeholder="admin@zhimart.com"
                  required
                  className="w-full pl-10 pr-4 py-3 bg-slate-50 border border-slate-200 rounded-xl text-sm font-semibold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500 focus:bg-white transition-all"
                />
              </div>
            </div>

            <div>
              <label className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5">
                Admin Access Key
              </label>
              <div className="relative">
                <Lock className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type={showPassword ? 'text' : 'password'}
                  value={loginPassword}
                  onChange={(e) => setLoginPassword(e.target.value)}
                  placeholder="••••••••"
                  required
                  className="w-full pl-10 pr-10 py-3 bg-slate-50 border border-slate-200 rounded-xl text-sm font-semibold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500 focus:bg-white transition-all"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword(!showPassword)}
                  className="absolute right-3.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600"
                >
                  {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                </button>
              </div>
            </div>

            <button
              type="submit"
              className="w-full py-3.5 px-4 bg-slate-950 hover:bg-slate-900 text-white font-extrabold text-sm rounded-xl transition-all shadow-md active:scale-98 flex items-center justify-center gap-2 mt-2"
            >
              <ShieldCheck className="w-4 h-4 text-amber-400" />
              <span>Authorize Admin Login</span>
            </button>
          </form>

          {/* 1-Click Demo Fill */}
          <div className="mt-5 pt-5 border-t border-slate-100 flex flex-col gap-2.5">
            <button
              type="button"
              onClick={handleQuickDemoLogin}
              className="w-full py-2.5 px-4 bg-amber-50 hover:bg-amber-100/80 border border-amber-200 text-amber-900 text-xs font-black rounded-xl transition-all text-center flex items-center justify-center gap-2"
            >
              <span>⚡ One-Click Demo Admin Login</span>
            </button>

            <button
              type="button"
              onClick={onBackToStore}
              className="w-full py-2 text-xs font-bold text-slate-500 hover:text-slate-900 transition-colors flex items-center justify-center gap-1.5"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
              <span>Back to Storefront</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  // LOGGED-IN ADMIN CONSOLE
  return (
    <div className="min-h-screen bg-[#F8F9FA] text-slate-900 flex flex-col text-left">
      {/* Toast Notification */}
      {toastMessage && (
        <div className="fixed bottom-6 right-6 z-50 bg-slate-950 text-white px-5 py-3 rounded-2xl shadow-xl flex items-center gap-2.5 text-xs font-bold border border-slate-800 animate-in fade-in slide-in-from-bottom-2 duration-150">
          <CheckCircle2 className="w-4 h-4 text-emerald-400" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Top Admin Navigation Header */}
      <header className="bg-white border-b border-slate-200/90 sticky top-0 z-30 shadow-xs">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-20 flex items-center justify-between gap-4">
          {/* Brand & Emblem */}
          <div className="flex items-center gap-3.5">
            <div className="w-10 h-10 rounded-2xl bg-gradient-to-tr from-slate-900 via-slate-800 to-amber-500 p-0.5 shadow-xs shrink-0">
              <div className="w-full h-full bg-slate-950 rounded-[14px] flex items-center justify-center">
                <span
                  className="font-black text-transparent bg-clip-text bg-gradient-to-tr from-amber-400 to-amber-200 text-lg leading-none"
                  style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                >
                  ழி
                </span>
              </div>
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="text-lg font-black tracking-tight text-slate-900 font-heading">
                  ZHI-MART
                </span>
                <span className="bg-amber-100 text-amber-900 text-[10px] font-black uppercase px-2 py-0.5 rounded-md border border-amber-200/60">
                  ADMIN CONSOLE
                </span>
              </div>
              <p className="text-[10px] font-bold text-slate-400">
                Sivakasi Depot Central Management System
              </p>
            </div>
          </div>

          {/* Right Header Controls */}
          <div className="flex items-center gap-3">
            <button
              onClick={onBackToStore}
              className="inline-flex items-center gap-1.5 text-xs font-bold text-slate-700 hover:text-slate-900 bg-slate-100 hover:bg-slate-200/80 px-3.5 py-2 rounded-xl transition-all"
              title="Return to customer catalog"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">Preview Storefront</span>
            </button>

            <div className="h-6 w-[1px] bg-slate-200 hidden sm:block" />

            <div className="flex items-center gap-2 bg-slate-50 border border-slate-200 px-3 py-1.5 rounded-xl">
              <div className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
              <span className="text-xs font-extrabold text-slate-700 hidden md:inline">
                admin@zhimart.com
              </span>
            </div>

            <button
              onClick={onLogout}
              className="inline-flex items-center gap-1.5 text-xs font-bold text-rose-600 hover:bg-rose-50 border border-rose-200 px-3 py-2 rounded-xl transition-all"
              title="Log out of admin session"
            >
              <LogOut className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">Logout</span>
            </button>
          </div>
        </div>

        {/* Tab Track */}
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex overflow-x-auto gap-2 py-2 border-t border-slate-100 no-scrollbar">
          <button
            onClick={() => setActiveTab('analytics')}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
              activeTab === 'analytics'
                ? 'bg-slate-950 text-white shadow-xs'
                : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
            }`}
          >
            <LayoutDashboard className="w-3.5 h-3.5" />
            <span>Analytical Dashboard</span>
          </button>

          <button
            onClick={() => setActiveTab('orders')}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
              activeTab === 'orders'
                ? 'bg-slate-950 text-white shadow-xs'
                : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
            }`}
          >
            <Package className="w-3.5 h-3.5" />
            <span>Orders & Sivakasi Dispatch</span>
            {pendingOrdersCount > 0 && (
              <span className="bg-amber-400 text-slate-950 text-[10px] font-black px-1.5 py-0.2 rounded-md">
                {pendingOrdersCount}
              </span>
            )}
          </button>

          <button
            onClick={() => setActiveTab('products')}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
              activeTab === 'products'
                ? 'bg-slate-950 text-white shadow-xs'
                : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
            }`}
          >
            <ShoppingBag className="w-3.5 h-3.5" />
            <span>Product Catalog (CRUD)</span>
            <span className="bg-slate-200 text-slate-700 text-[10px] font-black px-1.5 py-0.2 rounded-md">
              {products.length}
            </span>
          </button>

          <button
            onClick={() => setActiveTab('razorpay')}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
              activeTab === 'razorpay'
                ? 'bg-slate-950 text-white shadow-xs'
                : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
            }`}
          >
            <CreditCard className="w-3.5 h-3.5" />
            <span>Razorpay Payments</span>
            <span className="bg-emerald-100 text-emerald-800 text-[10px] font-black px-1.5 py-0.2 rounded-md">
              Live
            </span>
          </button>

          <button
            onClick={() => setActiveTab('inventory')}
            className={`flex items-center gap-2 px-4 py-2 rounded-xl text-xs font-bold whitespace-nowrap transition-all ${
              activeTab === 'inventory'
                ? 'bg-slate-950 text-white shadow-xs'
                : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
            }`}
          >
            <Warehouse className="w-3.5 h-3.5" />
            <span>Sivakasi Inventory & Bays</span>
            {lowStockProducts.length > 0 && (
              <span className="bg-rose-500 text-white text-[10px] font-black px-1.5 py-0.2 rounded-md">
                {lowStockProducts.length} Alert
              </span>
            )}
          </button>
        </div>
      </header>

      {/* Main Tab Content */}
      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 flex-1 w-full">
        {/* =========================================================================
            TAB 1: ANALYTICAL DASHBOARD
           ========================================================================= */}
        {activeTab === 'analytics' && (
          <div className="space-y-8 animate-in fade-in duration-150">
            {/* Top Header Summary */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div>
                <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
                  Sivakasi Operations Analytics
                </h2>
                <p className="text-xs text-slate-500 font-semibold mt-0.5">
                  Live pre-booking volume, gross sales, Sivakasi lorry transit and settlement telemetry.
                </p>
              </div>
              <div className="flex items-center gap-2">
                <span className="inline-flex items-center gap-1.5 text-xs font-bold text-emerald-700 bg-emerald-50 border border-emerald-200 px-3 py-1.5 rounded-xl">
                  <span className="w-2 h-2 rounded-full bg-emerald-500 animate-ping" />
                  Live Syncing • Today
                </span>
              </div>
            </div>

            {/* KPI Cards Grid */}
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 sm:gap-6">
              {/* Card 1: Gross Sales */}
              <div className="bg-white p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
                <div className="flex items-center justify-between text-slate-500 mb-2">
                  <span className="text-xs font-bold uppercase tracking-wider">Gross Sales Volume</span>
                  <div className="w-9 h-9 rounded-xl bg-amber-50 text-amber-700 flex items-center justify-center">
                    <DollarSign className="w-4 h-4" />
                  </div>
                </div>
                <div className="text-2xl sm:text-3xl font-black text-slate-950 tracking-tight font-heading">
                  ₹{totalRevenue.toLocaleString('en-IN')}
                </div>
                <div className="flex items-center gap-2 mt-3 text-xs font-bold text-emerald-600">
                  <TrendingUp className="w-3.5 h-3.5" />
                  <span>+28.4% vs last Diwali pre-season</span>
                </div>
              </div>

              {/* Card 2: Orders Processed */}
              <div className="bg-white p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
                <div className="flex items-center justify-between text-slate-500 mb-2">
                  <span className="text-xs font-bold uppercase tracking-wider">Total Consignments</span>
                  <div className="w-9 h-9 rounded-xl bg-blue-50 text-blue-700 flex items-center justify-center">
                    <Package className="w-4 h-4" />
                  </div>
                </div>
                <div className="text-2xl sm:text-3xl font-black text-slate-950 tracking-tight font-heading">
                  {totalOrdersCount} Orders
                </div>
                <div className="flex items-center gap-2 mt-3 text-xs font-semibold text-slate-500">
                  <span className="font-extrabold text-amber-600">{pendingOrdersCount} Pending Pack</span>
                  <span>•</span>
                  <span className="font-extrabold text-blue-600">{dispatchedOrdersCount} In Transit</span>
                </div>
              </div>

              {/* Card 3: Avg Order Value */}
              <div className="bg-white p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
                <div className="flex items-center justify-between text-slate-500 mb-2">
                  <span className="text-xs font-bold uppercase tracking-wider">Avg Order Value (AOV)</span>
                  <div className="w-9 h-9 rounded-xl bg-purple-50 text-purple-700 flex items-center justify-center">
                    <ShoppingBag className="w-4 h-4" />
                  </div>
                </div>
                <div className="text-2xl sm:text-3xl font-black text-slate-950 tracking-tight font-heading">
                  ₹2,464
                </div>
                <div className="flex items-center gap-2 mt-3 text-xs font-bold text-slate-500">
                  <span>Festival Hamper Combos</span>
                </div>
              </div>

              {/* Card 4: Razorpay Settlements */}
              <div className="bg-white p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
                <div className="flex items-center justify-between text-slate-500 mb-2">
                  <span className="text-xs font-bold uppercase tracking-wider">Razorpay Gateway SLA</span>
                  <div className="w-9 h-9 rounded-xl bg-emerald-50 text-emerald-700 flex items-center justify-center">
                    <CreditCard className="w-4 h-4" />
                  </div>
                </div>
                <div className="text-2xl sm:text-3xl font-black text-emerald-600 tracking-tight font-heading">
                  99.2%
                </div>
                <div className="flex items-center gap-2 mt-3 text-xs font-bold text-slate-500">
                  <span>Instant UPI & RuPay Settlement</span>
                </div>
              </div>
            </div>

            {/* Factory Command Shortcuts */}
            <div className="bg-white p-6 sm:p-8 rounded-3xl border border-slate-200/90 shadow-2xs">
              <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-6">
                <div>
                  <h3 className="text-base font-extrabold text-slate-950">
                    Factory Command Shortcuts
                  </h3>
                  <p className="text-xs text-slate-500 mt-0.5">
                    Direct access to primary dispatch, inventory, and catalog operations.
                  </p>
                </div>
                <div className="text-xs font-bold text-slate-600 bg-slate-50 border border-slate-200 px-3.5 py-1.5 rounded-xl">
                  Sivakasi Warehouse Dispatch Bay 1 • 24/7 Peak Operations
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
                <button
                  onClick={() => {
                    setActiveTab('products');
                    handleOpenAddModal();
                  }}
                  className="p-5 rounded-2xl bg-slate-950 hover:bg-slate-900 text-white text-xs font-black transition-all flex flex-col justify-between gap-3 shadow-xs group text-left"
                >
                  <div className="w-9 h-9 rounded-xl bg-slate-800 text-amber-400 flex items-center justify-center">
                    <Plus className="w-5 h-5 group-hover:rotate-90 transition-transform" />
                  </div>
                  <div>
                    <span className="text-sm font-extrabold text-white block">Add New Cracker</span>
                    <span className="text-[11px] text-slate-400 font-medium">Create SKU with 45% discount</span>
                  </div>
                </button>

                <button
                  onClick={() => {
                    setActiveTab('orders');
                    setOrderFilter('pending_pack');
                  }}
                  className="p-5 rounded-2xl bg-amber-50 hover:bg-amber-100/80 border border-amber-200/80 text-amber-950 text-xs font-black transition-all flex flex-col justify-between gap-3 text-left"
                >
                  <div className="w-9 h-9 rounded-xl bg-amber-200/60 text-amber-800 flex items-center justify-center">
                    <Clock className="w-5 h-5" />
                  </div>
                  <div>
                    <span className="text-sm font-extrabold text-slate-950 block">Process Sivakasi Packing</span>
                    <span className="text-[11px] text-amber-800 font-semibold">{pendingOrdersCount} orders waiting in bay</span>
                  </div>
                </button>

                <button
                  onClick={() => setActiveTab('inventory')}
                  className="p-5 rounded-2xl bg-slate-50 hover:bg-slate-100 border border-slate-200 text-slate-900 text-xs font-black transition-all flex flex-col justify-between gap-3 text-left"
                >
                  <div className="w-9 h-9 rounded-xl bg-slate-200 text-slate-700 flex items-center justify-center">
                    <Warehouse className="w-5 h-5" />
                  </div>
                  <div>
                    <span className="text-sm font-extrabold text-slate-950 block">Inspect Sivakasi Bays</span>
                    <span className="text-[11px] text-slate-500 font-medium">Bay A, B, C & D stock matrix</span>
                  </div>
                </button>

                <button
                  onClick={() => setActiveTab('razorpay')}
                  className="p-5 rounded-2xl bg-slate-50 hover:bg-slate-100 border border-slate-200 text-slate-900 text-xs font-black transition-all flex flex-col justify-between gap-3 text-left"
                >
                  <div className="w-9 h-9 rounded-xl bg-emerald-100 text-emerald-800 flex items-center justify-center">
                    <CreditCard className="w-5 h-5" />
                  </div>
                  <div>
                    <span className="text-sm font-extrabold text-slate-950 block">Razorpay Gateways</span>
                    <span className="text-[11px] text-emerald-700 font-semibold">99.2% captured settlement</span>
                  </div>
                </button>
              </div>
            </div>
          </div>
        )}

        {/* =========================================================================
            TAB 2: ORDERS STATUS PROVIDINGS
           ========================================================================= */}
        {activeTab === 'orders' && (
          <div className="space-y-6 animate-in fade-in duration-150">
            {/* Header + Filters */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div>
                <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
                  Orders & Sivakasi Dispatch Tracking
                </h2>
                <p className="text-xs text-slate-500 font-semibold mt-0.5">
                  Update order fulfillment status, assign transport carriers (VRL/ABT/ARC), and docket numbers.
                </p>
              </div>

              {/* Search Order */}
              <div className="relative w-full sm:w-72">
                <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={orderSearch}
                  onChange={(e) => setOrderSearch(e.target.value)}
                  placeholder="Search by Order ID, name, or phone..."
                  className="w-full pl-10 pr-4 py-2.5 bg-white border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>
            </div>

            {/* Filter Tabs */}
            <div className="flex flex-wrap gap-2 pb-2">
              {[
                { key: 'all', label: `All Orders (${orders.length})` },
                { key: 'pending_pack', label: `Pending Pack (${orders.filter(o => o.status === 'pending_pack').length})` },
                { key: 'dispatched', label: `Dispatched (${orders.filter(o => o.status === 'dispatched').length})` },
                { key: 'in_transit', label: `In Transit (${orders.filter(o => o.status === 'in_transit').length})` },
                { key: 'delivered', label: `Delivered (${orders.filter(o => o.status === 'delivered').length})` },
              ].map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => setOrderFilter(tab.key as any)}
                  className={`px-3.5 py-2 rounded-xl text-xs font-bold transition-all ${
                    orderFilter === tab.key
                      ? 'bg-slate-950 text-white shadow-2xs'
                      : 'bg-white text-slate-700 hover:bg-slate-100 border border-slate-200'
                  }`}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            {/* Orders List */}
            <div className="space-y-4">
              {filteredOrders.length === 0 ? (
                <div className="bg-white p-12 text-center rounded-3xl border border-slate-200">
                  <Package className="w-10 h-10 text-slate-300 mx-auto mb-3" />
                  <p className="text-sm font-extrabold text-slate-800">No orders found matching filters</p>
                  <p className="text-xs text-slate-500 mt-1">Try resetting search or filter options</p>
                </div>
              ) : (
                filteredOrders.map((order) => {
                  const getStatusBadge = () => {
                    switch (order.status) {
                      case 'pending_pack':
                        return {
                          bg: 'bg-amber-50 text-amber-800 border-amber-200',
                          label: 'Pending Factory Pack',
                          icon: Clock,
                        };
                      case 'dispatched':
                        return {
                          bg: 'bg-blue-50 text-blue-800 border-blue-200',
                          label: 'Dispatched from Sivakasi',
                          icon: Truck,
                        };
                      case 'in_transit':
                        return {
                          bg: 'bg-purple-50 text-purple-800 border-purple-200',
                          label: 'In Transit (Lorry Transport)',
                          icon: Truck,
                        };
                      case 'out_for_delivery':
                        return {
                          bg: 'bg-orange-50 text-orange-800 border-orange-200',
                          label: 'Out For Delivery',
                          icon: Truck,
                        };
                      case 'delivered':
                        return {
                          bg: 'bg-emerald-50 text-emerald-800 border-emerald-200',
                          label: 'Delivered',
                          icon: CheckCircle2,
                        };
                    }
                  };
                  const badge = getStatusBadge();
                  const StatusIcon = badge.icon;

                  return (
                    <div
                      key={order.id}
                      className="bg-white rounded-3xl border border-slate-200 p-6 shadow-2xs hover:shadow-xs transition-shadow"
                    >
                      {/* Top Bar of Order Card */}
                      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-4 border-b border-slate-100">
                        <div className="flex flex-wrap items-center gap-3">
                          <button
                            onClick={() => setSelectedOrderForInvoice(order)}
                            className="text-base font-black text-slate-950 font-mono tracking-tight hover:text-amber-600 transition-colors flex items-center gap-1.5 cursor-pointer group"
                            title="View official order estimation & print PDF"
                          >
                            <span>{order.id}</span>
                            <Printer className="w-3.5 h-3.5 text-slate-400 group-hover:text-amber-500 transition-colors" />
                          </button>
                          <span className="text-xs font-bold text-slate-400">
                            {order.date}
                          </span>
                          <span className="text-[11px] font-bold bg-slate-100 text-slate-700 px-2.5 py-1 rounded-lg">
                            {order.paymentMethod}
                          </span>
                        </div>

                        {/* Current Status Pill */}
                        <div className="flex items-center gap-2">
                          <span
                            className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-black border ${badge.bg}`}
                          >
                            <StatusIcon className="w-3.5 h-3.5" />
                            <span>{badge.label}</span>
                          </span>
                        </div>
                      </div>

                      {/* Middle Details */}
                      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 py-4">
                        {/* Customer Information */}
                        <div>
                          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1">
                            Customer Details
                          </p>
                          <p className="text-sm font-black text-slate-900">{order.customerName}</p>
                          <p className="text-xs font-semibold text-slate-600 mt-0.5">{order.customerPhone}</p>
                          <p className="text-xs text-slate-500 mt-1 leading-relaxed">
                            {order.customerAddress}
                          </p>
                        </div>

                        {/* Itemized Order Summary */}
                        <div className="md:col-span-1">
                          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1">
                            Consignment Items ({order.items.length})
                          </p>
                          <div className="space-y-1.5">
                            {order.items.map((it, idx) => (
                              <div key={idx} className="flex justify-between text-xs font-semibold">
                                <span className="text-slate-800">
                                  {it.quantity}x {it.productName}
                                </span>
                                <span className="text-slate-950 font-bold">
                                  ₹{it.price * it.quantity}
                                </span>
                              </div>
                            ))}
                          </div>
                          <div className="mt-2 pt-2 border-t border-slate-100 flex justify-between text-xs font-black">
                            <span>Total Payable:</span>
                            <span className="text-amber-600 font-heading text-sm">
                              ₹{order.totalAmount}
                            </span>
                          </div>
                        </div>

                        {/* Transport Logistics & Docket */}
                        <div>
                          <p className="text-[10px] font-extrabold uppercase tracking-wider text-slate-400 mb-1">
                            Sivakasi Transport & Docket
                          </p>
                          <div className="p-3 bg-slate-50 rounded-2xl border border-slate-200/80 space-y-1">
                            <div className="flex justify-between text-xs">
                              <span className="text-slate-500 font-bold">Carrier:</span>
                              <span className="font-extrabold text-slate-900">
                                {order.transportPartner || 'VRL Logistics'}
                              </span>
                            </div>
                            <div className="flex justify-between text-xs">
                              <span className="text-slate-500 font-bold">Docket / LR:</span>
                              <span className="font-mono font-black text-blue-700">
                                {order.docketNumber || 'Unassigned'}
                              </span>
                            </div>
                            {order.dispatchedAt && (
                              <p className="text-[10px] font-semibold text-slate-500 pt-1">
                                {order.dispatchedAt}
                              </p>
                            )}
                          </div>

                          <button
                            onClick={() => handleOpenDocketModal(order)}
                            className="mt-2 text-xs font-bold text-blue-600 hover:text-blue-800 flex items-center gap-1"
                          >
                            <Edit3 className="w-3.5 h-3.5" />
                            <span>Edit Carrier / Docket No.</span>
                          </button>
                        </div>
                      </div>

                      {/* Bottom Status Providings Control Bar */}
                      <div className="pt-4 border-t border-slate-100 flex flex-wrap items-center justify-between gap-3">
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-bold text-slate-500">Provide Status:</span>
                          <select
                            value={order.status}
                            onChange={(e) => {
                              onUpdateOrderStatus(order.id, e.target.value as OrderStatus);
                              showToast(`Status updated to ${e.target.value.replace('_', ' ')} for ${order.id}`);
                            }}
                            className="bg-white border border-slate-300 text-slate-900 text-xs font-extrabold rounded-xl px-3 py-2 focus:outline-none focus:ring-2 focus:ring-amber-500"
                          >
                            <option value="pending_pack">Pending Factory Pack</option>
                            <option value="dispatched">Dispatched from Sivakasi</option>
                            <option value="in_transit">In Transit (Lorry Transport)</option>
                            <option value="out_for_delivery">Out for Delivery</option>
                            <option value="delivered">Delivered to Customer</option>
                          </select>
                        </div>

                        <div className="flex flex-wrap items-center gap-2">
                          {/* View Estimation & Print PDF Button */}
                          <button
                            onClick={() => setSelectedOrderForInvoice(order)}
                            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-black bg-slate-950 hover:bg-slate-900 text-white shadow-xs transition-all active:scale-98 cursor-pointer"
                            title="View official Sivakasi order estimation & print PDF"
                          >
                            <Printer className="w-3.5 h-3.5 text-amber-400" />
                            <span>Estimation & Print PDF</span>
                          </button>

                          {/* WhatsApp Update Ping */}
                          <button
                            onClick={() => handleSendWhatsAppUpdate(order)}
                            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl text-xs font-bold bg-emerald-50 hover:bg-emerald-100 text-emerald-800 border border-emerald-200 transition-all active:scale-98 cursor-pointer"
                          >
                            <MessageCircle className="w-3.5 h-3.5 text-emerald-600" />
                            <span>Send WhatsApp LR Update</span>
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}

        {/* =========================================================================
            TAB 3: PRODUCT CATALOG (CRUD OPERATIONS)
           ========================================================================= */}
        {activeTab === 'products' && (
          <div className="space-y-6 animate-in fade-in duration-150">
            {/* Header + Add Product Button */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div>
                <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
                  Product Management (CRUD)
                </h2>
                <p className="text-xs text-slate-500 font-semibold mt-0.5">
                  Add new Sivakasi crackers, modify retail pricing (45% direct discount auto-calc), edit stock and remove products.
                </p>
              </div>

              <button
                onClick={handleOpenAddModal}
                className="inline-flex items-center gap-2 bg-slate-950 hover:bg-slate-900 text-white font-extrabold text-xs px-4 py-2.5 rounded-2xl transition-all shadow-xs active:scale-98"
              >
                <Plus className="w-4 h-4 text-amber-400" />
                <span>Add New Cracker Product</span>
              </button>
            </div>

            {/* Filter & Search Bar */}
            <div className="flex flex-col sm:flex-row gap-3">
              <div className="relative flex-1">
                <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={productSearch}
                  onChange={(e) => setProductSearch(e.target.value)}
                  placeholder="Search by product name, Tamil title, or category..."
                  className="w-full pl-10 pr-4 py-2.5 bg-white border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>

              <select
                value={productCategoryFilter}
                onChange={(e) => setProductCategoryFilter(e.target.value)}
                className="bg-white border border-slate-200 text-slate-800 text-xs font-bold rounded-xl px-3.5 py-2.5 focus:outline-none focus:ring-2 focus:ring-amber-500"
              >
                <option value="all">All Categories ({products.length})</option>
                {allCategories.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </div>

            {/* Products Table */}
            <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden shadow-2xs">
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse">
                  <thead>
                    <tr className="bg-slate-50 border-b border-slate-200 text-[11px] font-extrabold uppercase tracking-wider text-slate-500">
                      <th className="py-4 px-4 sm:px-6">Product & Image</th>
                      <th className="py-4 px-4">Category</th>
                      <th className="py-4 px-4">Pack Info</th>
                      <th className="py-4 px-4">Retail MRP</th>
                      <th className="py-4 px-4">Zhi-mart (45% Off)</th>
                      <th className="py-4 px-4">Sivakasi Bay / Stock</th>
                      <th className="py-4 px-4 sm:px-6 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100 text-xs">
                    {filteredProducts.map((p) => (
                      <tr key={p.id} className="hover:bg-slate-50/70 transition-colors">
                        {/* Image + Title */}
                        <td className="py-3.5 px-4 sm:px-6">
                          <div className="flex items-center gap-3">
                            <div className="w-12 h-12 rounded-xl bg-slate-100 border border-slate-200 overflow-hidden shrink-0">
                              <img
                                src={p.image}
                                alt={p.name}
                                className="w-full h-full object-cover"
                                onError={(e) => {
                                  (e.target as HTMLImageElement).src = '/assets/products/lakshmi_box.jpg';
                                }}
                              />
                            </div>
                            <div>
                              <p className="font-extrabold text-slate-900 leading-tight">
                                {p.name}
                              </p>
                              {p.tamilName && (
                                <p
                                  className="text-[11px] font-bold text-amber-700"
                                  style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                                >
                                  {p.tamilName}
                                </p>
                              )}
                              <p className="text-[10px] text-slate-400 font-mono mt-0.5">
                                SKU: {p.id}
                              </p>
                            </div>
                          </div>
                        </td>

                        {/* Category */}
                        <td className="py-3.5 px-4">
                          <span className="inline-block bg-slate-100 text-slate-700 text-[10px] font-extrabold px-2 py-0.5 rounded-md">
                            {p.category}
                          </span>
                        </td>

                        {/* Pack */}
                        <td className="py-3.5 px-4 font-semibold text-slate-600">
                          {p.packInfo}
                        </td>

                        {/* Retail MRP */}
                        <td className="py-3.5 px-4 text-slate-400 line-through font-semibold">
                          ₹{p.originalPrice}
                        </td>

                        {/* Zhi-mart Direct */}
                        <td className="py-3.5 px-4">
                          <div className="flex items-center gap-1.5">
                            <span className="font-black text-slate-950 font-heading text-sm">
                              ₹{p.price}
                            </span>
                            <span className="text-[10px] font-extrabold bg-red-100 text-red-800 px-1.5 py-0.2 rounded">
                              45% OFF
                            </span>
                          </div>
                        </td>

                        {/* Bay & Stock */}
                        <td className="py-3.5 px-4">
                          <div>
                            <span className="font-bold text-slate-800 text-xs">
                              {p.stock ?? 100} units
                            </span>
                            {(p.stock ?? 100) < 30 && (
                              <span className="ml-1.5 text-[9px] font-black bg-rose-100 text-rose-700 px-1.5 py-0.2 rounded uppercase">
                                Low Stock
                              </span>
                            )}
                            <p className="text-[10px] text-slate-400">
                              {p.bayLocation || 'Bay B-1'}
                            </p>
                          </div>
                        </td>

                        {/* Actions */}
                        <td className="py-3.5 px-4 sm:px-6 text-right">
                          <div className="flex items-center justify-end gap-2">
                            <button
                              onClick={() => handleOpenEditModal(p)}
                              className="p-2 rounded-xl text-slate-600 hover:text-slate-950 hover:bg-slate-100 transition-colors"
                              title="Edit product details"
                            >
                              <Edit3 className="w-4 h-4" />
                            </button>
                            <button
                              onClick={() => setDeletingProductId(p.id)}
                              className="p-2 rounded-xl text-rose-600 hover:text-rose-800 hover:bg-rose-50 transition-colors"
                              title="Delete product"
                            >
                              <Trash2 className="w-4 h-4" />
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}

        {/* =========================================================================
            TAB 4: RAZORPAY PAYMENT TRANSACTIONS
           ========================================================================= */}
        {activeTab === 'razorpay' && (
          <div className="space-y-6 animate-in fade-in duration-150">
            {/* Header + Gateway Status Banner */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div>
                <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
                  Razorpay Payment Transactions
                </h2>
                <p className="text-xs text-slate-500 font-semibold mt-0.5">
                  Direct gateway logs, captured settlements, automated MDR reconciliation, and refunds.
                </p>
              </div>

              <div className="flex items-center gap-2">
                <span className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-emerald-50 text-emerald-800 border border-emerald-200 text-xs font-bold">
                  <span className="w-2 h-2 rounded-full bg-emerald-500" />
                  Razorpay Production Gateway Live
                </span>
              </div>
            </div>

            {/* Financial Reconciliation Cards */}
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <div className="bg-white p-5 rounded-2xl border border-slate-200">
                <span className="text-[10px] font-extrabold uppercase text-slate-400">Total Settled Volume</span>
                <p className="text-2xl font-black text-slate-950 mt-1 font-heading">
                  ₹{totalRevenue.toLocaleString('en-IN')}
                </p>
                <p className="text-[11px] font-semibold text-emerald-600 mt-1">
                  Transferred to Sivakasi State Bank A/C
                </p>
              </div>

              <div className="bg-white p-5 rounded-2xl border border-slate-200">
                <span className="text-[10px] font-extrabold uppercase text-slate-400">Razorpay MDR Fee (2%)</span>
                <p className="text-2xl font-black text-slate-700 mt-1 font-heading">
                  ₹{Math.round(totalRevenue * 0.02).toLocaleString('en-IN')}
                </p>
                <p className="text-[11px] font-semibold text-slate-400 mt-1">
                  Includes 18% GST input tax credit
                </p>
              </div>

              <div className="bg-white p-5 rounded-2xl border border-slate-200">
                <span className="text-[10px] font-extrabold uppercase text-slate-400">Net Merchant Payout</span>
                <p className="text-2xl font-black text-emerald-600 mt-1 font-heading">
                  ₹{Math.round(totalRevenue * 0.98).toLocaleString('en-IN')}
                </p>
                <p className="text-[11px] font-semibold text-slate-500 mt-1">
                  T+1 Settlement SLA Guaranteed
                </p>
              </div>
            </div>

            {/* Filter Tabs & Search */}
            <div className="flex flex-col sm:flex-row justify-between gap-3">
              <div className="flex flex-wrap gap-2">
                {[
                  { key: 'all', label: `All Transactions (${transactions.length})` },
                  { key: 'captured', label: `Captured (${transactions.filter(t => t.status === 'captured').length})` },
                  { key: 'refunded', label: `Refunded (${transactions.filter(t => t.status === 'refunded').length})` },
                  { key: 'failed', label: `Failed (${transactions.filter(t => t.status === 'failed').length})` },
                ].map((tab) => (
                  <button
                    key={tab.key}
                    onClick={() => setRazorpayFilter(tab.key as any)}
                    className={`px-3.5 py-2 rounded-xl text-xs font-bold transition-all ${
                      razorpayFilter === tab.key
                        ? 'bg-slate-950 text-white shadow-2xs'
                        : 'bg-white text-slate-700 hover:bg-slate-100 border border-slate-200'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              <div className="relative w-full sm:w-72">
                <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={razorpaySearch}
                  onChange={(e) => setRazorpaySearch(e.target.value)}
                  placeholder="Search Payment ID, order, phone..."
                  className="w-full pl-10 pr-4 py-2 bg-white border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>
            </div>

            {/* Transactions Table */}
            <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden shadow-2xs">
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse">
                  <thead>
                    <tr className="bg-slate-50 border-b border-slate-200 text-[11px] font-extrabold uppercase tracking-wider text-slate-500">
                      <th className="py-4 px-4 sm:px-6">Razorpay Payment ID</th>
                      <th className="py-4 px-4">Customer</th>
                      <th className="py-4 px-4">Method & Route</th>
                      <th className="py-4 px-4">Amount</th>
                      <th className="py-4 px-4">Status</th>
                      <th className="py-4 px-4">Time & RRN</th>
                      <th className="py-4 px-4 sm:px-6 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100 text-xs">
                    {filteredTransactions.map((tx) => (
                      <tr key={tx.id} className="hover:bg-slate-50/70 transition-colors">
                        <td className="py-3.5 px-4 sm:px-6">
                          <p className="font-mono font-black text-slate-900">{tx.id}</p>
                          <p className="font-mono text-[10px] text-slate-400">{tx.orderId}</p>
                        </td>

                        <td className="py-3.5 px-4">
                          <p className="font-extrabold text-slate-900">{tx.customerName}</p>
                          <p className="text-[11px] text-slate-500">{tx.customerPhone}</p>
                        </td>

                        <td className="py-3.5 px-4">
                          <span className="inline-block bg-slate-100 text-slate-800 text-[10px] font-black px-2 py-0.5 rounded-md mb-0.5">
                            {tx.method}
                          </span>
                          <p className="text-[11px] font-medium text-slate-500">{tx.methodDetail}</p>
                        </td>

                        <td className="py-3.5 px-4">
                          <span className="font-black text-slate-950 font-heading text-sm">
                            ₹{tx.amount.toLocaleString('en-IN')}
                          </span>
                        </td>

                        <td className="py-3.5 px-4">
                          {tx.status === 'captured' ? (
                            <span className="inline-flex items-center gap-1 text-[11px] font-black bg-emerald-50 text-emerald-800 border border-emerald-200 px-2.5 py-0.5 rounded-md">
                              <CheckCircle2 className="w-3 h-3" />
                              Captured
                            </span>
                          ) : tx.status === 'refunded' ? (
                            <span className="inline-flex items-center gap-1 text-[11px] font-black bg-slate-100 text-slate-800 border border-slate-200 px-2.5 py-0.5 rounded-md">
                              Refunded
                            </span>
                          ) : (
                            <span className="inline-flex items-center gap-1 text-[11px] font-black bg-rose-50 text-rose-800 border border-rose-200 px-2.5 py-0.5 rounded-md">
                              Failed
                            </span>
                          )}
                        </td>

                        <td className="py-3.5 px-4">
                          <p className="text-slate-700 font-semibold">{tx.createdAt}</p>
                          <p className="font-mono text-[10px] text-slate-400">RRN: {tx.rrn}</p>
                        </td>

                        <td className="py-3.5 px-4 sm:px-6 text-right">
                          <div className="flex items-center justify-end gap-2">
                            <button
                              onClick={() => setViewingReceipt(tx)}
                              className="px-3 py-1.5 rounded-xl text-xs font-bold bg-slate-100 hover:bg-slate-200 text-slate-800 transition-colors"
                            >
                              Receipt
                            </button>
                            {tx.status === 'captured' && (
                              <button
                                onClick={() => {
                                  if (confirm(`Refund payment ₹${tx.amount} to ${tx.customerName}?`)) {
                                    onRefundTransaction(tx.id);
                                    showToast(`Refund processed for ${tx.id}`);
                                  }
                                }}
                                className="px-2.5 py-1.5 rounded-xl text-[11px] font-bold text-rose-700 hover:bg-rose-50 border border-rose-200 transition-colors"
                              >
                                Refund
                              </button>
                            )}
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}

        {/* =========================================================================
            TAB 5: INVENTORY MANAGEMENT & FACTORY BAYS
           ========================================================================= */}
        {activeTab === 'inventory' && (
          <div className="space-y-6 animate-in fade-in duration-150">
            {/* Header */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div>
                <h2 className="text-2xl font-black text-slate-950 tracking-tight font-heading">
                  Sivakasi Inventory & Warehouse Bays
                </h2>
                <p className="text-xs text-slate-500 font-semibold mt-0.5">
                  Real-time stock ledger, safety bunker allocations, and quick batch restocking.
                </p>
              </div>

              <div className="relative w-full sm:w-72">
                <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={inventorySearch}
                  onChange={(e) => setInventorySearch(e.target.value)}
                  placeholder="Search stock by product name..."
                  className="w-full pl-10 pr-4 py-2.5 bg-white border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>
            </div>

            {/* Warehouse Bays Showcase */}
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div className="bg-white p-5 rounded-3xl border border-slate-200">
                <div className="flex justify-between items-center mb-2">
                  <span className="text-[10px] font-black uppercase text-amber-700 bg-amber-50 px-2 py-0.5 rounded">
                    Bay A-1 / Aerial Bunker
                  </span>
                  <Warehouse className="w-4 h-4 text-slate-400" />
                </div>
                <p className="text-xl font-black text-slate-950">420 Units</p>
                <p className="text-[11px] text-slate-500 mt-1">Multi-shot cakes & single sky shells</p>
              </div>

              <div className="bg-white p-5 rounded-3xl border border-slate-200">
                <div className="flex justify-between items-center mb-2">
                  <span className="text-[10px] font-black uppercase text-red-700 bg-red-50 px-2 py-0.5 rounded">
                    Bay B-1 / Sound Bunker
                  </span>
                  <Warehouse className="w-4 h-4 text-slate-400" />
                </div>
                <p className="text-xl font-black text-slate-950">890 Units</p>
                <p className="text-[11px] text-slate-500 mt-1">Lakshmi, Deluxe & Bird single sound</p>
              </div>

              <div className="bg-white p-5 rounded-3xl border border-slate-200">
                <div className="flex justify-between items-center mb-2">
                  <span className="text-[10px] font-black uppercase text-blue-700 bg-blue-50 px-2 py-0.5 rounded">
                    Bay C-1 / Sparklers Hall
                  </span>
                  <Warehouse className="w-4 h-4 text-slate-400" />
                </div>
                <p className="text-xl font-black text-slate-950">640 Units</p>
                <p className="text-[11px] text-slate-500 mt-1">Electric sparklers & ground chakras</p>
              </div>

              <div className="bg-white p-5 rounded-3xl border border-slate-200">
                <div className="flex justify-between items-center mb-2">
                  <span className="text-[10px] font-black uppercase text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded">
                    Bay D-1 / Hamper Packaging
                  </span>
                  <Warehouse className="w-4 h-4 text-slate-400" />
                </div>
                <p className="text-xl font-black text-slate-950">150 Units</p>
                <p className="text-[11px] text-slate-500 mt-1">Festival 25-in-1 family grand boxes</p>
              </div>
            </div>

            {/* Low Stock Warning Alert if any */}
            {lowStockProducts.length > 0 && (
              <div className="p-4 rounded-2xl bg-rose-50 border border-rose-200 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 text-rose-900 text-xs font-bold">
                <div className="flex items-center gap-2">
                  <AlertTriangle className="w-4 h-4 text-rose-600 shrink-0" />
                  <span>
                    Attention: {lowStockProducts.length} cracker items are running below the 30 unit safety threshold!
                  </span>
                </div>
                <button
                  onClick={() => {
                    lowStockProducts.forEach((p) => onUpdateStock(p.id, (p.stock ?? 0) + 50));
                    showToast('Replenished 50 units each for all low stock items from Sivakasi factory!');
                  }}
                  className="px-3 py-1.5 bg-rose-600 hover:bg-rose-700 text-white rounded-xl text-xs font-black transition-colors shrink-0 shadow-2xs"
                >
                  ⚡ Fast Restock All (+50 Units)
                </button>
              </div>
            )}

            {/* Inventory Table with Inline Stock Adjuster */}
            <div className="bg-white rounded-3xl border border-slate-200 overflow-hidden shadow-2xs">
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse">
                  <thead>
                    <tr className="bg-slate-50 border-b border-slate-200 text-[11px] font-extrabold uppercase tracking-wider text-slate-500">
                      <th className="py-4 px-4 sm:px-6">Product</th>
                      <th className="py-4 px-4">Warehouse Bay</th>
                      <th className="py-4 px-4">Current Stock</th>
                      <th className="py-4 px-4">Status</th>
                      <th className="py-4 px-4 sm:px-6 text-right">Quick Stock Restock</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100 text-xs">
                    {products
                      .filter((p) =>
                        p.name.toLowerCase().includes(inventorySearch.toLowerCase())
                      )
                      .map((p) => {
                        const currentUnits = p.stock ?? 100;
                        return (
                          <tr key={p.id} className="hover:bg-slate-50/70 transition-colors">
                            <td className="py-3.5 px-4 sm:px-6">
                              <p className="font-extrabold text-slate-900">{p.name}</p>
                              <p className="text-[10px] text-slate-400 font-mono">
                                {p.category} • {p.id}
                              </p>
                            </td>

                            <td className="py-3.5 px-4 font-semibold text-slate-600">
                              {p.bayLocation || 'Bay B-1 (Sound Bunker)'}
                            </td>

                            <td className="py-3.5 px-4">
                              <span className="font-black text-slate-950 font-heading text-sm">
                                {currentUnits} units
                              </span>
                            </td>

                            <td className="py-3.5 px-4">
                              {currentUnits < 30 ? (
                                <span className="inline-block px-2.5 py-0.5 rounded-md text-[10px] font-black bg-rose-100 text-rose-800">
                                  CRITICAL LOW
                                </span>
                              ) : currentUnits < 60 ? (
                                <span className="inline-block px-2.5 py-0.5 rounded-md text-[10px] font-black bg-amber-100 text-amber-800">
                                  MODERATE
                                </span>
                              ) : (
                                <span className="inline-block px-2.5 py-0.5 rounded-md text-[10px] font-black bg-emerald-100 text-emerald-800">
                                  HEALTHY
                                </span>
                              )}
                            </td>

                            <td className="py-3.5 px-4 sm:px-6 text-right">
                              <div className="flex items-center justify-end gap-1.5">
                                <button
                                  onClick={() => {
                                    onUpdateStock(p.id, Math.max(0, currentUnits - 10));
                                    showToast(`Deducted 10 units for ${p.name}`);
                                  }}
                                  className="w-7 h-7 rounded-lg bg-slate-100 hover:bg-slate-200 text-slate-700 font-black text-xs flex items-center justify-center transition-colors"
                                  title="Deduct 10 units"
                                >
                                  -10
                                </button>
                                <button
                                  onClick={() => {
                                    onUpdateStock(p.id, currentUnits + 10);
                                    showToast(`Added 10 units for ${p.name}`);
                                  }}
                                  className="w-7 h-7 rounded-lg bg-slate-100 hover:bg-slate-200 text-slate-700 font-black text-xs flex items-center justify-center transition-colors"
                                  title="Add 10 units"
                                >
                                  +10
                                </button>
                                <button
                                  onClick={() => {
                                    onUpdateStock(p.id, currentUnits + 50);
                                    showToast(`Batch restocked +50 units for ${p.name}`);
                                  }}
                                  className="px-2.5 py-1 rounded-lg bg-slate-950 hover:bg-slate-800 text-white font-extrabold text-[11px] transition-colors"
                                  title="Batch restock +50 units"
                                >
                                  +50 Batch
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </main>

      {/* =========================================================================
          MODAL 1: ADD / EDIT PRODUCT MODAL (CRUD)
         ========================================================================= */}
      {(isAddModalOpen || editingProduct) && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4 overflow-y-auto">
          <div className="w-full max-w-2xl bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 sm:p-8 animate-in fade-in zoom-in-95 duration-150 my-8">
            <div className="flex items-center justify-between pb-4 border-b border-slate-100">
              <h3 className="text-lg font-black text-slate-950 font-heading">
                {editingProduct ? 'Edit Cracker Product' : 'Add New Sivakasi Cracker'}
              </h3>
              <button
                onClick={() => {
                  setIsAddModalOpen(false);
                  setEditingProduct(null);
                }}
                className="p-1 rounded-xl text-slate-400 hover:text-slate-700 hover:bg-slate-100"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleSaveProduct} className="space-y-4 pt-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                {/* Product Name */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Product Title (English) *
                  </label>
                  <input
                    type="text"
                    value={formName}
                    onChange={(e) => setFormName(e.target.value)}
                    placeholder="e.g. 50-Shot Matrix Aerial Cake"
                    required
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>

                {/* Tamil Name */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Tamil Title (தமிழ்)
                  </label>
                  <input
                    type="text"
                    value={formTamilName}
                    onChange={(e) => setFormTamilName(e.target.value)}
                    placeholder="e.g. 50 ஷாட் வண்ண வாணவேடிக்கை"
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                    style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                  />
                </div>

                {/* Category */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Category *
                  </label>
                  <select
                    value={formCategory}
                    onChange={(e) => setFormCategory(e.target.value)}
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  >
                    <option value="SINGLE SOUND CRACKERS">SINGLE SOUND CRACKERS</option>
                    <option value="AERIAL DISPLAY SHOTS">AERIAL DISPLAY SHOTS</option>
                    <option value="SPARKLERS & FOUNTAINS">SPARKLERS & FOUNTAINS</option>
                    <option value="GROUND CHAKRAS">GROUND CHAKRAS</option>
                    <option value="FLOWER POTS & CONES">FLOWER POTS & CONES</option>
                    <option value="FESTIVAL GIFT HAMPERS">FESTIVAL GIFT HAMPERS</option>
                  </select>
                </div>

                {/* Pack Info */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Pack Specification *
                  </label>
                  <input
                    type="text"
                    value={formPackInfo}
                    onChange={(e) => setFormPackInfo(e.target.value)}
                    placeholder="e.g. 10 Packet(s) or 1 Box(es)"
                    required
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>

                {/* Original Retail Price */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Retail MRP (₹) *
                  </label>
                  <input
                    type="number"
                    value={formOriginalPrice}
                    onChange={(e) => {
                      const val = e.target.value;
                      setFormOriginalPrice(val);
                      const num = parseFloat(val);
                      if (num > 0) {
                        setFormPrice(Math.round(num * 0.55).toString());
                      }
                    }}
                    placeholder="e.g. 400"
                    required
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>

                {/* Direct Zhi-mart Price */}
                <div>
                  <div className="flex justify-between items-center mb-1">
                    <label className="text-xs font-bold text-slate-700">
                      Zhi-mart Price (₹) *
                    </label>
                    <span className="text-[10px] font-bold text-amber-600">
                      45% Off Sivakasi Price
                    </span>
                  </div>
                  <input
                    type="number"
                    value={formPrice}
                    onChange={(e) => setFormPrice(e.target.value)}
                    placeholder="e.g. 220"
                    required
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>

                {/* Initial Stock */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Warehouse In-Stock Units *
                  </label>
                  <input
                    type="number"
                    value={formStock}
                    onChange={(e) => setFormStock(e.target.value)}
                    placeholder="e.g. 100"
                    required
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>

                {/* Sivakasi Bay Location */}
                <div>
                  <label className="block text-xs font-bold text-slate-700 mb-1">
                    Sivakasi Bunker / Bay
                  </label>
                  <input
                    type="text"
                    value={formBayLocation}
                    onChange={(e) => setFormBayLocation(e.target.value)}
                    placeholder="e.g. Bay A-2 (Aerial Shells)"
                    className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                  />
                </div>
              </div>

              {/* Preset Image Selector */}
              <div>
                <label className="block text-xs font-bold text-slate-700 mb-1.5">
                  Select Product Image Asset
                </label>
                <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
                  {PRESET_PRODUCT_IMAGES.map((img) => (
                    <button
                      type="button"
                      key={img.url}
                      onClick={() => setFormImage(img.url)}
                      className={`p-2 rounded-xl border text-left flex flex-col gap-1 transition-all ${
                        formImage === img.url
                          ? 'border-amber-500 bg-amber-50/50 ring-2 ring-amber-400/20'
                          : 'border-slate-200 hover:border-slate-300'
                      }`}
                    >
                      <div className="w-full h-12 rounded-lg bg-slate-100 overflow-hidden">
                        <img src={img.url} alt={img.label} className="w-full h-full object-cover" />
                      </div>
                      <span className="text-[10px] font-bold text-slate-700 truncate">
                        {img.label}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              {/* Description */}
              <div>
                <label className="block text-xs font-bold text-slate-700 mb-1">
                  Product Description & Safety
                </label>
                <textarea
                  value={formDescription}
                  onChange={(e) => setFormDescription(e.target.value)}
                  rows={2}
                  placeholder="Manufactured with certified green pyrotechnic formula..."
                  className="w-full px-3.5 py-2 bg-slate-50 border border-slate-200 rounded-xl text-xs font-medium text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>

              {/* Submit Buttons */}
              <div className="pt-4 border-t border-slate-100 flex items-center justify-end gap-3">
                <button
                  type="button"
                  onClick={() => {
                    setIsAddModalOpen(false);
                    setEditingProduct(null);
                  }}
                  className="px-4 py-2.5 rounded-xl text-xs font-bold text-slate-600 hover:bg-slate-100 transition-colors"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-6 py-2.5 rounded-xl text-xs font-black bg-slate-950 hover:bg-slate-900 text-white transition-all shadow-md active:scale-98"
                >
                  {editingProduct ? 'Save Changes' : 'Publish Product to Store'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* =========================================================================
          MODAL 2: DELETE PRODUCT CONFIRMATION
         ========================================================================= */}
      {deletingProductId && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center gap-3 text-rose-600 mb-3">
              <div className="w-10 h-10 rounded-2xl bg-rose-50 flex items-center justify-center">
                <Trash2 className="w-5 h-5" />
              </div>
              <h3 className="text-base font-extrabold text-slate-950">
                Confirm Product Deletion
              </h3>
            </div>
            <p className="text-xs text-slate-600 leading-relaxed mb-6">
              Are you sure you want to remove this product from the Zhi-mart catalog? It will be delisted from the customer store immediately.
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setDeletingProductId(null)}
                className="px-4 py-2 text-xs font-bold text-slate-600 hover:bg-slate-100 rounded-xl transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onDeleteProduct(deletingProductId);
                  setDeletingProductId(null);
                  showToast('Product successfully removed from catalog');
                }}
                className="px-5 py-2 text-xs font-black bg-rose-600 hover:bg-rose-700 text-white rounded-xl transition-all shadow-xs"
              >
                Delete Product
              </button>
            </div>
          </div>
        </div>
      )}

      {/* =========================================================================
          MODAL 3: EDIT DOCKET / TRANSPORT MODAL
         ========================================================================= */}
      {selectedOrderForDocket && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center justify-between pb-3 border-b border-slate-100 mb-4">
              <h3 className="text-base font-black text-slate-950">
                Assign Carrier & Docket No.
              </h3>
              <button
                onClick={() => setSelectedOrderForDocket(null)}
                className="p-1 rounded-xl text-slate-400 hover:text-slate-700"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handleSaveDocket} className="space-y-4">
              <div>
                <label className="block text-xs font-bold text-slate-700 mb-1">
                  Transport Partner
                </label>
                <select
                  value={editTransportPartner}
                  onChange={(e) => setEditTransportPartner(e.target.value)}
                  className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500"
                >
                  <option value="VRL Logistics">VRL Logistics</option>
                  <option value="ABT Parcel Service">ABT Parcel Service</option>
                  <option value="ARC Transports">ARC Transports</option>
                  <option value="KPN Speed Parcel">KPN Speed Parcel</option>
                  <option value="Sivakasi Direct Express">Sivakasi Direct Express</option>
                </select>
              </div>

              <div>
                <label className="block text-xs font-bold text-slate-700 mb-1">
                  Docket / Lorry Receipt (LR) Number
                </label>
                <input
                  type="text"
                  value={editDocketNo}
                  onChange={(e) => setEditDocketNo(e.target.value)}
                  placeholder="LR-VRL-xxxx"
                  required
                  className="w-full px-3.5 py-2.5 bg-slate-50 border border-slate-200 rounded-xl text-xs font-bold text-slate-900 font-mono focus:outline-none focus:ring-2 focus:ring-amber-500"
                />
              </div>

              <div className="pt-2 flex justify-end gap-2.5">
                <button
                  type="button"
                  onClick={() => setSelectedOrderForDocket(null)}
                  className="px-4 py-2 text-xs font-bold text-slate-600 hover:bg-slate-100 rounded-xl"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-5 py-2 text-xs font-black bg-slate-950 text-white hover:bg-slate-900 rounded-xl shadow-xs"
                >
                  Save Docket
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* =========================================================================
          MODAL 4: RAZORPAY RECEIPT DETAIL MODAL
         ========================================================================= */}
      {viewingReceipt && (
        <div className="fixed inset-0 z-50 bg-slate-900/60 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-slate-200 p-6 sm:p-7 animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center justify-between pb-3 border-b border-slate-100 mb-4">
              <div className="flex items-center gap-2">
                <span className="font-extrabold text-blue-600 text-sm">RAZORPAY</span>
                <span className="text-xs font-bold text-slate-400">• Official Payment Receipt</span>
              </div>
              <button
                onClick={() => setViewingReceipt(null)}
                className="p-1 rounded-xl text-slate-400 hover:text-slate-700"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="text-center py-4 bg-slate-50 rounded-2xl border border-slate-100 mb-4">
              <p className="text-[10px] font-extrabold uppercase text-slate-400">Total Captured Amount</p>
              <p className="text-3xl font-black text-slate-950 mt-1 font-heading">
                ₹{viewingReceipt.amount.toLocaleString('en-IN')}
              </p>
              <span className="inline-block mt-1 px-2.5 py-0.5 rounded-full text-[10px] font-black bg-emerald-100 text-emerald-800">
                Payment Successful • Captured
              </span>
            </div>

            <div className="space-y-2 text-xs divide-y divide-slate-100">
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Payment ID:</span>
                <span className="font-mono font-black text-slate-900">{viewingReceipt.id}</span>
              </div>
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Razorpay Order ID:</span>
                <span className="font-mono font-bold text-slate-700">{viewingReceipt.orderId}</span>
              </div>
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Customer:</span>
                <span className="font-bold text-slate-900">{viewingReceipt.customerName}</span>
              </div>
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Method:</span>
                <span className="font-bold text-slate-800">{viewingReceipt.methodDetail}</span>
              </div>
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Bank RRN:</span>
                <span className="font-mono font-bold text-slate-700">{viewingReceipt.rrn}</span>
              </div>
              <div className="flex justify-between py-1.5">
                <span className="text-slate-500 font-semibold">Settlement Account:</span>
                <span className="font-semibold text-slate-700">SBI Sivakasi (•••• 4912)</span>
              </div>
            </div>

            <div className="pt-5 mt-4 border-t border-slate-100 flex justify-end">
              <button
                onClick={() => setViewingReceipt(null)}
                className="w-full py-2.5 bg-slate-950 text-white font-black text-xs rounded-xl hover:bg-slate-900 transition-colors"
              >
                Close Receipt
              </button>
            </div>
          </div>
        </div>
      )}

      {/* =========================================================================
          MODAL 5: SIVAKASI ESTIMATION & DISPATCH INVOICE (PRINT / PDF)
         ========================================================================= */}
      {selectedOrderForInvoice && (
        <div className="fixed inset-0 z-50 bg-slate-950/80 backdrop-blur-xs flex items-center justify-center p-2 sm:p-4 overflow-y-auto">
          <div className="w-full max-w-4xl bg-slate-100 rounded-3xl shadow-2xl border border-slate-300 overflow-hidden animate-in fade-in zoom-in-95 duration-150 my-6 flex flex-col max-h-[92vh]">
            {/* Top Action Bar (hidden during browser print) */}
            <div className="no-print bg-slate-900 text-white px-6 py-4 flex flex-wrap items-center justify-between gap-3 border-b border-slate-800 shrink-0">
              <div className="flex items-center gap-3">
                <div className="w-9 h-9 rounded-xl bg-amber-500/20 text-amber-400 flex items-center justify-center">
                  <Printer className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="text-sm font-black text-white font-heading">
                    Order Estimation Slip • {selectedOrderForInvoice.id}
                  </h3>
                  <p className="text-[11px] font-semibold text-slate-400">
                    Customer: {selectedOrderForInvoice.customerName} ({selectedOrderForInvoice.customerCity})
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-2">
                {/* Print / Save as PDF Button */}
                <button
                  onClick={() => window.print()}
                  className="px-4 py-2.5 bg-amber-500 hover:bg-amber-400 text-slate-950 font-black text-xs rounded-xl transition-all shadow-sm flex items-center gap-2 active:scale-97 cursor-pointer"
                  title="Print slip or save as PDF"
                >
                  <Printer className="w-4 h-4" />
                  <span>Download PDF / Print Slip</span>
                </button>

                {/* WhatsApp Share Copy */}
                <button
                  onClick={() => handleSendWhatsAppUpdate(selectedOrderForInvoice)}
                  className="px-3.5 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white font-bold text-xs rounded-xl transition-all flex items-center gap-1.5 active:scale-97 cursor-pointer"
                  title="Share details via WhatsApp"
                >
                  <MessageCircle className="w-4 h-4" />
                  <span className="hidden sm:inline">WhatsApp Copy</span>
                </button>

                {/* Close Button */}
                <button
                  onClick={() => setSelectedOrderForInvoice(null)}
                  className="p-2 text-slate-400 hover:text-white hover:bg-slate-800 rounded-xl transition-colors cursor-pointer"
                  title="Close estimation modal"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>
            </div>

            {/* Scrollable Printable Document Container */}
            <div className="overflow-y-auto p-4 sm:p-8 bg-slate-200/60 flex-1">
              <div
                id="printable-estimation-slip"
                className="bg-white rounded-2xl border border-slate-300 p-6 sm:p-10 shadow-lg text-slate-900 mx-auto max-w-3xl font-sans"
              >
                {/* 1. Official Header */}
                <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4 pb-6 border-b-2 border-slate-900">
                  <div className="flex items-start gap-3.5">
                    <div className="w-12 h-12 rounded-2xl bg-slate-950 flex items-center justify-center shrink-0 shadow-xs border border-amber-500/30">
                      <span
                        className="font-black text-amber-400 text-2xl leading-none"
                        style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}
                      >
                        ழி
                      </span>
                    </div>
                    <div>
                      <h1 className="text-xl sm:text-2xl font-black text-slate-950 tracking-tight font-heading leading-tight">
                        ZHI-MART FIREWORKS
                      </h1>
                      <p className="text-[11px] font-extrabold uppercase tracking-wider text-amber-700">
                        Direct Factory Consignment Depot • Sivakasi
                      </p>
                      <p className="text-[10px] text-slate-500 font-medium leading-relaxed mt-0.5 max-w-md">
                        124/2 Bypass Road, Sivakasi - 626123, Virudhunagar Dist, Tamil Nadu, India<br />
                        Helpline: +91 98400 12345 • Email: sales@zhimartcrackers.com<br />
                        <span className="font-bold text-slate-700">GSTIN: 33AAACZ8912P1Z0</span> • <span className="font-bold text-slate-700">PESO Lic: EXP/TN/2024/9810</span>
                      </p>
                    </div>
                  </div>

                  <div className="text-left sm:text-right shrink-0">
                    <span className="inline-block bg-slate-900 text-amber-400 text-[10px] font-black uppercase px-2.5 py-1 rounded-md tracking-wider">
                      ORDER ESTIMATION & INVOICE
                    </span>
                    <p className="font-mono font-black text-base text-slate-950 mt-1.5">
                      {selectedOrderForInvoice.id}
                    </p>
                    <p className="text-[11px] text-slate-500 font-semibold">
                      Date: {selectedOrderForInvoice.date}
                    </p>
                    <span className="inline-block mt-1 text-[10px] font-black px-2 py-0.5 rounded bg-slate-100 text-slate-800 uppercase">
                      Status: {selectedOrderForInvoice.status.replace('_', ' ')}
                    </span>
                  </div>
                </div>

                {/* 2. Customer & Dispatch Meta Grid */}
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-6 py-6 border-b border-slate-200 text-xs">
                  {/* Billed & Shipped To */}
                  <div className="bg-slate-50/80 p-4 rounded-xl border border-slate-200">
                    <p className="text-[10px] font-black uppercase tracking-wider text-slate-400 mb-2">
                      Consignment Destination & Customer
                    </p>
                    <p className="text-sm font-black text-slate-950">
                      {selectedOrderForInvoice.customerName}
                    </p>
                    <p className="text-slate-600 font-bold mt-0.5">
                      Phone: {selectedOrderForInvoice.customerPhone}
                    </p>
                    <p className="text-slate-700 font-semibold mt-1 leading-relaxed">
                      {selectedOrderForInvoice.customerAddress}
                    </p>
                    <p className="text-slate-900 font-extrabold mt-1">
                      City: {selectedOrderForInvoice.customerCity}
                    </p>
                  </div>

                  {/* Dispatch & Carrier Logistics */}
                  <div className="bg-slate-50/80 p-4 rounded-xl border border-slate-200">
                    <p className="text-[10px] font-black uppercase tracking-wider text-slate-400 mb-2">
                      Sivakasi Transport & Payment
                    </p>
                    <div className="space-y-1">
                      <div className="flex justify-between">
                        <span className="text-slate-500 font-bold">Transport Carrier:</span>
                        <span className="font-extrabold text-slate-900">
                          {selectedOrderForInvoice.transportPartner || 'VRL Logistics'}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-slate-500 font-bold">Docket / LR Number:</span>
                        <span className="font-mono font-black text-blue-700">
                          {selectedOrderForInvoice.docketNumber || 'Generating at Sivakasi Bay'}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-slate-500 font-bold">Payment Method:</span>
                        <span className="font-bold text-slate-900">{selectedOrderForInvoice.paymentMethod}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-slate-500 font-bold">Payment Ref ID:</span>
                        <span className="font-mono text-[11px] text-slate-700">
                          {selectedOrderForInvoice.paymentId || 'pay_SVK88910a'}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-slate-500 font-bold">Dispatch Depot:</span>
                        <span className="font-semibold text-slate-700">Sivakasi Factory Bay 1</span>
                      </div>
                    </div>
                  </div>
                </div>

                {/* 3. Itemized Products Table */}
                <div className="py-6">
                  <table className="w-full text-left border-collapse text-xs">
                    <thead>
                      <tr className="bg-slate-900 text-white text-[10px] font-black uppercase tracking-wider">
                        <th className="py-2.5 px-3 rounded-l-lg">#</th>
                        <th className="py-2.5 px-3">Cracker Item Description</th>
                        <th className="py-2.5 px-3 text-center">Qty</th>
                        <th className="py-2.5 px-3 text-right">Factory Direct Rate</th>
                        <th className="py-2.5 px-3 text-right rounded-r-lg">Amount (₹)</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-slate-200">
                      {selectedOrderForInvoice.items.map((item, index) => (
                        <tr key={index} className="hover:bg-slate-50/50">
                          <td className="py-3 px-3 font-bold text-slate-400">{index + 1}</td>
                          <td className="py-3 px-3">
                            <p className="font-extrabold text-slate-900">{item.productName}</p>
                            <p className="text-[10px] text-slate-400 font-mono">SKU: {item.productId}</p>
                          </td>
                          <td className="py-3 px-3 text-center font-bold text-slate-800">
                            {item.quantity}
                          </td>
                          <td className="py-3 px-3 text-right font-semibold text-slate-600">
                            ₹{item.price}
                          </td>
                          <td className="py-3 px-3 text-right font-black text-slate-950 font-heading">
                            ₹{item.price * item.quantity}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>

                {/* 4. Financial Summary & Discount Calculation */}
                <div className="py-4 border-t-2 border-slate-200 grid grid-cols-1 sm:grid-cols-2 gap-6 items-start text-xs">
                  {/* Left: Certification & Remarks */}
                  <div className="space-y-2 text-slate-500 text-[11px] leading-relaxed">
                    <p className="font-extrabold text-slate-800 uppercase tracking-wider text-[10px]">
                      Sivakasi Factory Certification
                    </p>
                    <p>
                      • 100% genuine Green Crackers with CSIR-NEERI certified chemical composition.<br />
                      • High-decibel sound crackers and aerial display items meet PESO statutory noise limits.<br />
                      • Delivered directly from Sivakasi factory bay to transport booking parcel hub.
                    </p>
                    <div className="mt-3 p-2.5 rounded-lg bg-amber-50 border border-amber-200/80 text-amber-900 text-[10px] font-bold">
                      Safety Warning: Always ignite crackers in open outdoor grounds under adult supervision.
                    </div>
                  </div>

                  {/* Right: Calculations */}
                  <div className="bg-slate-50 p-4 rounded-xl border border-slate-200 space-y-2">
                    <div className="flex justify-between text-slate-600">
                      <span>Original Retail MRP:</span>
                      <span className="font-semibold text-slate-700 line-through">
                        ₹{Math.round(selectedOrderForInvoice.totalAmount / 0.55).toLocaleString('en-IN')}
                      </span>
                    </div>

                    <div className="flex justify-between text-emerald-700 font-bold">
                      <span>Sivakasi Direct Festival Discount (45%):</span>
                      <span>
                        -₹{(Math.round(selectedOrderForInvoice.totalAmount / 0.55) - selectedOrderForInvoice.totalAmount).toLocaleString('en-IN')}
                      </span>
                    </div>

                    <div className="flex justify-between text-slate-600">
                      <span>Sivakasi Doorstep Delivery:</span>
                      <span className="font-bold text-emerald-600">FREE</span>
                    </div>

                    <div className="pt-2 border-t border-slate-200 flex justify-between items-center text-sm font-black text-slate-950">
                      <span>TOTAL PAYABLE AMOUNT:</span>
                      <span className="text-base text-amber-600 font-heading">
                        ₹{selectedOrderForInvoice.totalAmount.toLocaleString('en-IN')}
                      </span>
                    </div>

                    <p className="text-[10px] text-slate-500 font-medium pt-1 text-right italic">
                      Amount in words: {numberToWords(selectedOrderForInvoice.totalAmount)}
                    </p>
                  </div>
                </div>

                {/* 5. Authorized Signatures & Factory Stamp */}
                <div className="pt-8 mt-6 border-t border-slate-200 flex flex-row items-end justify-between gap-4">
                  {/* Customer Signature Box */}
                  <div className="text-center w-40">
                    <div className="h-12 border-b border-dashed border-slate-400 mb-1" />
                    <p className="text-[10px] font-bold text-slate-500 uppercase tracking-wider">
                      Customer Signature
                    </p>
                  </div>

                  {/* Factory Stamp & Authorized Signatory */}
                  <div className="text-center flex flex-col items-center">
                    <div className="w-16 h-16 rounded-full border-2 border-dashed border-amber-600 flex flex-col items-center justify-center p-1 text-[8px] font-black text-amber-800 leading-tight mb-1 bg-amber-50/50">
                      <span>ZHI-MART</span>
                      <span className="text-xs font-black">ழி</span>
                      <span>SIVAKASI</span>
                    </div>
                    <div className="h-4 border-b border-slate-400 w-36 mb-1" />
                    <p className="text-[10px] font-black text-slate-900 uppercase tracking-wider">
                      Authorized Factory Dispatch
                    </p>
                    <p className="text-[9px] text-slate-400">
                      Sivakasi Pyrotechnics Depot
                    </p>
                  </div>
                </div>
              </div>
            </div>

            {/* Bottom Footer inside modal */}
            <div className="no-print bg-white px-6 py-3.5 border-t border-slate-200 flex flex-wrap items-center justify-between gap-2 shrink-0">
              <span className="text-xs text-slate-500 font-semibold">
                Tip: Click <strong>Download PDF / Print Slip</strong> and select <em>Save as PDF</em> in the printer destination to download.
              </span>
              <button
                onClick={() => setSelectedOrderForInvoice(null)}
                className="px-4 py-2 bg-slate-900 text-white rounded-xl text-xs font-black hover:bg-slate-800 transition-colors"
              >
                Close Estimation Slip
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
