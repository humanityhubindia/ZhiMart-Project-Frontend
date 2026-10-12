import React from 'react';
import {
  Truck,
  MapPin,
  ShieldCheck,
  ArrowDown,
  FileSpreadsheet,
  Bot,
  Percent,
  CheckCircle2,
  Clock,
  PackageCheck,
  Building2,
  BadgeCheck,
} from 'lucide-react';

interface HeroSectionProps {
  onExploreProducts: () => void;
  onOpenQuickEstimate: () => void;
  onOpenAIAssistant: () => void;
}

export const HeroSection: React.FC<HeroSectionProps> = ({
  onExploreProducts,
  onOpenQuickEstimate,
  onOpenAIAssistant,
}) => {
  return (
    <section className="relative w-full bg-gradient-to-b from-white via-slate-50/70 to-[#F8F9FA] border-b border-slate-200/80 pt-10 pb-14 sm:pt-14 sm:pb-16 px-4 sm:px-6 lg:px-8 text-left">
      <div className="max-w-7xl mx-auto space-y-10">
        {/* Main 2-Column Showcase */}
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-10 items-center">
          {/* Left Column (7 cols): Editorial Copy & Primary Actions */}
          <div className="lg:col-span-7 space-y-6">
            {/* Top Unified Pill Tag */}
            <div className="inline-flex items-center gap-2.5 bg-slate-900 text-white px-4 py-2 rounded-full text-xs font-bold shadow-xs">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              <span>Direct From Sivakasi, Tamil Nadu</span>
              <span className="text-slate-600">&bull;</span>
              <span className="text-amber-300 font-extrabold">Free Doorstep Delivery</span>
            </div>

            {/* Main Headline */}
            <h1 className="text-3xl sm:text-5xl lg:text-6xl font-black text-slate-950 tracking-tight font-heading leading-[1.12]">
              Crackers Directly From{' '}
              <span className="text-amber-600">Sivakasi</span> To Your Doorstep.
            </h1>

            {/* Sub-headline */}
            <p className="text-sm sm:text-base lg:text-lg text-slate-600 font-medium leading-relaxed max-w-2xl">
              Order fresh 2025 factory fireworks dispatched straight from certified manufacturing units in Sivakasi.
              Zero retail markups, zero middleman margins — enjoy <strong className="text-slate-900 font-extrabold">100% Free Insured Delivery</strong> and a <strong className="text-slate-900 font-extrabold">Flat 45% Factory Wholesale Discount</strong>.
            </p>

            {/* Quick Action Buttons */}
            <div className="flex flex-wrap items-center gap-3 pt-2">
              <button
                type="button"
                onClick={onExploreProducts}
                className="inline-flex items-center gap-2 bg-slate-950 hover:bg-slate-800 active:scale-97 text-white font-extrabold text-xs sm:text-sm px-7 py-4 rounded-xl transition-all shadow-sm hover:shadow"
              >
                <span>Explore All Crackers</span>
                <ArrowDown className="w-4 h-4 text-amber-400" />
              </button>

              <button
                type="button"
                onClick={onOpenQuickEstimate}
                className="inline-flex items-center gap-2 bg-white hover:bg-slate-50 active:scale-97 text-slate-800 font-extrabold text-xs sm:text-sm px-6 py-4 rounded-xl border border-slate-300 transition-all shadow-2xs hover:border-slate-400"
              >
                <FileSpreadsheet className="w-4 h-4 text-amber-600" />
                <span>Quick Rate Sheet</span>
                <span className="text-[10px] bg-red-600 text-white font-black px-1.5 py-0.2 rounded-md uppercase tracking-wide">
                  HOT
                </span>
              </button>

              <button
                type="button"
                onClick={onOpenAIAssistant}
                className="inline-flex items-center gap-2 bg-white hover:bg-slate-50 active:scale-97 text-slate-800 font-extrabold text-xs sm:text-sm px-6 py-4 rounded-xl border border-slate-300 transition-all shadow-2xs hover:border-slate-400"
              >
                <Bot className="w-4 h-4 text-slate-600" />
                <span>AI Budget Curator</span>
              </button>
            </div>

            {/* Trust bullet checkpoints */}
            <div className="flex flex-wrap items-center gap-y-2 gap-x-6 pt-1 text-xs font-bold text-slate-700">
              <div className="flex items-center gap-2">
                <CheckCircle2 className="w-4 h-4 text-emerald-600 shrink-0" />
                <span>Zero Middleman Markups</span>
              </div>
              <div className="flex items-center gap-2">
                <CheckCircle2 className="w-4 h-4 text-emerald-600 shrink-0" />
                <span>100% Free Insured Transit</span>
              </div>
              <div className="flex items-center gap-2">
                <CheckCircle2 className="w-4 h-4 text-emerald-600 shrink-0" />
                <span>PESO Certified Green Crackers</span>
              </div>
            </div>
          </div>

          {/* Right Column (5 cols): High-End Factory Guarantee Pass */}
          <div className="lg:col-span-5">
            <div className="bg-white rounded-3xl border border-slate-200/90 shadow-md p-6 sm:p-7 space-y-6 relative overflow-hidden">
              {/* Card Header with Tamil Emblem */}
              <div className="flex items-center justify-between border-b border-slate-100 pb-4">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-2xl bg-slate-950 text-amber-400 flex items-center justify-center font-black text-base shadow-xs">
                    <span style={{ fontFamily: "'Noto Sans Tamil', system-ui, sans-serif" }}>
                      ழி
                    </span>
                  </div>
                  <div>
                    <h3 className="text-sm font-black text-slate-900 leading-tight">
                      ZHI-MART Direct Hub
                    </h3>
                    <p className="text-[11px] text-slate-400 font-semibold">
                      Bypass Road, Sivakasi, Tamil Nadu
                    </p>
                  </div>
                </div>

                <span className="text-[10px] font-extrabold uppercase tracking-wider bg-emerald-50 text-emerald-800 px-2.5 py-1 rounded-full border border-emerald-200">
                  Direct Dispatch
                </span>
              </div>

              {/* Verified Metrics Grid */}
              <div className="grid grid-cols-2 gap-3 text-left">
                <div className="p-3.5 bg-slate-50/80 rounded-2xl border border-slate-200/80 space-y-1">
                  <span className="text-[10px] font-bold text-slate-400 uppercase tracking-wider block">
                    Doorstep Delivery
                  </span>
                  <span className="text-base font-black text-slate-900 block">
                    FREE (₹0)
                  </span>
                  <span className="text-[10px] text-emerald-600 font-bold block">
                    Insured Cargo Transport
                  </span>
                </div>

                <div className="p-3.5 bg-slate-50/80 rounded-2xl border border-slate-200/80 space-y-1">
                  <span className="text-[10px] font-bold text-slate-400 uppercase tracking-wider block">
                    Festive Pricing
                  </span>
                  <span className="text-base font-black text-slate-900 block">
                    Flat 45% OFF
                  </span>
                  <span className="text-[10px] text-emerald-600 font-bold block">
                    Factory Wholesale Rates
                  </span>
                </div>

                <div className="p-3.5 bg-slate-50/80 rounded-2xl border border-slate-200/80 space-y-1">
                  <span className="text-[10px] font-bold text-slate-400 uppercase tracking-wider block">
                    Origin Verification
                  </span>
                  <span className="text-base font-black text-slate-900 block">
                    100% Sivakasi
                  </span>
                  <span className="text-[10px] text-slate-500 font-bold block">
                    Fresh 2025 Production
                  </span>
                </div>

                <div className="p-3.5 bg-slate-50/80 rounded-2xl border border-slate-200/80 space-y-1">
                  <span className="text-[10px] font-bold text-slate-400 uppercase tracking-wider block">
                    Safety Standard
                  </span>
                  <span className="text-base font-black text-slate-900 block">
                    PESO Licensed
                  </span>
                  <span className="text-[10px] text-slate-500 font-bold block">
                    Tested Decibel Safe
                  </span>
                </div>
              </div>

              {/* Bottom Guarantee Banner */}
              <div className="p-3 bg-amber-50/70 border border-amber-200/80 rounded-2xl flex items-center gap-3 text-xs text-amber-900">
                <BadgeCheck className="w-5 h-5 text-amber-600 shrink-0" />
                <p className="font-semibold leading-tight">
                  Guaranteed genuine green crackers with safe Visco delay fuses and tamper-proof factory packaging.
                </p>
              </div>
            </div>
          </div>
        </div>

        {/* 4 Value Pillars: Consistent, Unified Slate & Amber Cards */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 pt-4 border-t border-slate-200/70">
          {/* Card 1 */}
          <div className="bg-white rounded-2xl border border-slate-200/80 p-5 shadow-2xs hover:shadow-xs hover:border-slate-300 transition-all space-y-2.5">
            <div className="w-10 h-10 rounded-xl bg-slate-100 border border-slate-200 text-slate-900 flex items-center justify-center font-bold">
              <MapPin className="w-5 h-5 text-amber-600" />
            </div>
            <h3 className="text-sm font-black text-slate-900 tracking-tight">
              Direct From Sivakasi
            </h3>
            <p className="text-xs text-slate-500 font-medium leading-relaxed">
              Every order is packed and dispatched directly from our Sivakasi manufacturing hub. Zero middleman fees.
            </p>
          </div>

          {/* Card 2 */}
          <div className="bg-white rounded-2xl border border-slate-200/80 p-5 shadow-2xs hover:shadow-xs hover:border-slate-300 transition-all space-y-2.5">
            <div className="w-10 h-10 rounded-xl bg-slate-100 border border-slate-200 text-slate-900 flex items-center justify-center font-bold">
              <Truck className="w-5 h-5 text-emerald-600" />
            </div>
            <h3 className="text-sm font-black text-slate-900 tracking-tight">
              Free Doorstep Delivery
            </h3>
            <p className="text-xs text-slate-500 font-medium leading-relaxed">
              100% free delivery across India with insured fireworks cargo transit partners and live docket tracking.
            </p>
          </div>

          {/* Card 3 */}
          <div className="bg-white rounded-2xl border border-slate-200/80 p-5 shadow-2xs hover:shadow-xs hover:border-slate-300 transition-all space-y-2.5">
            <div className="w-10 h-10 rounded-xl bg-slate-100 border border-slate-200 text-slate-900 flex items-center justify-center font-bold">
              <Percent className="w-5 h-5 text-blue-600" />
            </div>
            <h3 className="text-sm font-black text-slate-900 tracking-tight">
              Flat 45% Factory Rates
            </h3>
            <p className="text-xs text-slate-500 font-medium leading-relaxed">
              Wholesale factory rate cards applied automatically. Save up to half compared to local retail fireworks stalls.
            </p>
          </div>

          {/* Card 4 */}
          <div className="bg-white rounded-2xl border border-slate-200/80 p-5 shadow-2xs hover:shadow-xs hover:border-slate-300 transition-all space-y-2.5">
            <div className="w-10 h-10 rounded-xl bg-slate-100 border border-slate-200 text-slate-900 flex items-center justify-center font-bold">
              <ShieldCheck className="w-5 h-5 text-indigo-600" />
            </div>
            <h3 className="text-sm font-black text-slate-900 tracking-tight">
              100% PESO Certified
            </h3>
            <p className="text-xs text-slate-500 font-medium leading-relaxed">
              Government authorized green crackers featuring tested Visco delay fuses and decibel compliant acoustics.
            </p>
          </div>
        </div>
      </div>
    </section>
  );
};
