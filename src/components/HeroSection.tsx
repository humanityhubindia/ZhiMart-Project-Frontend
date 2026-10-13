import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
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
  Building2,
  BadgeCheck,
} from 'lucide-react';

interface HeroSectionProps {
  onExploreProducts: () => void;
  onOpenQuickEstimate: () => void;
  onOpenAIAssistant: () => void;
}

interface TrustedPerson {
  id: string;
  image: string;
  title: string;
  badge: string;
  subtitle: string;
  quote: string;
  highlight: string;
  objectPosition: string;
}

const TRUSTED_PERSONS: TrustedPerson[] = [
  {
    id: 'person-1',
    image: '/assets/founders/founder_senior.jpg',
    title: 'Trusted By Them',
    badge: 'Trusted By Them',
    subtitle: '30+ Years Sivakasi Trust',
    quote: '“Direct from Sivakasi manufacturing units with authentic green fireworks certification and genuine factory wholesale rates.”',
    highlight: 'Sivakasi Industry Veteran',
    objectPosition: 'object-center',
  },
  {
    id: 'person-2',
    image: '/assets/founders/founder_young.jpg',
    title: 'Trusted By Them',
    badge: 'Trusted By Them',
    subtitle: 'Factory-Direct Patron',
    quote: '“Zero retail markups and transparent pricing — Sivakasi’s freshest crackers safely delivered with 100% free shipping.”',
    highlight: 'Direct-To-Home Advocate',
    objectPosition: 'object-center',
  },
];

export const HeroSection: React.FC<HeroSectionProps> = ({
  onExploreProducts,
  onOpenQuickEstimate,
  onOpenAIAssistant,
}) => {
  const [currentIdx, setCurrentIdx] = useState(0);
  const [isPaused, setIsPaused] = useState(false);

  useEffect(() => {
    if (isPaused) return;
    const timer = setInterval(() => {
      setCurrentIdx((prev) => (prev + 1) % TRUSTED_PERSONS.length);
    }, 4000);
    return () => clearInterval(timer);
  }, [isPaused]);

  const currentPerson = TRUSTED_PERSONS[currentIdx];

  return (
    <section className="relative w-full bg-gradient-to-b from-white via-slate-50/70 to-[#F8F9FA] border-b border-slate-200/80 pt-10 pb-14 sm:pt-14 sm:pb-16 px-4 sm:px-6 lg:px-8 text-left">
      <div className="max-w-7xl mx-auto space-y-12">
        {/* Main 2-Column Showcase */}
        <div className="grid grid-cols-1 md:grid-cols-12 gap-8 lg:gap-12 items-center">
          {/* Left Column (7 cols): Editorial Copy & Primary Actions */}
          <div className="md:col-span-7 space-y-6">
            {/* Top Unified Pill Tag */}
            <div className="inline-flex items-center gap-2 bg-slate-950 text-white px-3.5 py-1.5 sm:px-4 sm:py-2 rounded-full text-xs font-bold shadow-xs border border-slate-800">
              <span className="text-slate-200">Direct From Sivakasi, Tamil Nadu</span>
              <span className="text-slate-600">&bull;</span>
              <span className="text-amber-400 font-extrabold flex items-center gap-1.5">
                <Truck className="w-3.5 h-3.5 text-amber-400 shrink-0 inline" />
                Free Delivery
              </span>
            </div>

            {/* Main Headline */}
            <h1 className="text-3xl sm:text-4xl md:text-5xl lg:text-5xl font-black text-slate-950 tracking-tight font-heading leading-[1.14]">
              Crackers Directly From{' '}
              <span className="text-amber-600">Sivakasi</span> With Free Delivery.
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
                className="inline-flex items-center gap-2 bg-slate-950 hover:bg-slate-800 active:scale-97 text-white font-extrabold text-xs sm:text-sm px-6 sm:px-7 py-3.5 sm:py-4 rounded-xl transition-all shadow-md hover:shadow-lg"
              >
                <span>Explore All Crackers</span>
                <ArrowDown className="w-4 h-4 text-amber-400" />
              </button>

              <button
                type="button"
                onClick={onOpenQuickEstimate}
                className="inline-flex items-center gap-2 bg-white hover:bg-slate-50 active:scale-97 text-slate-900 font-extrabold text-xs sm:text-sm px-5 sm:px-6 py-3.5 sm:py-4 rounded-xl border border-slate-300 transition-all shadow-2xs hover:border-slate-400"
              >
                <FileSpreadsheet className="w-4 h-4 text-amber-600" />
                <span>Quick Rate Sheet</span>
                <span className="text-[10px] bg-red-600 text-white font-black px-1.5 py-0.5 rounded-md uppercase tracking-wider">
                  HOT
                </span>
              </button>

              <button
                type="button"
                onClick={onOpenAIAssistant}
                className="inline-flex items-center gap-2 bg-white hover:bg-slate-50 active:scale-97 text-slate-900 font-extrabold text-xs sm:text-sm px-5 sm:px-6 py-3.5 sm:py-4 rounded-xl border border-slate-300 transition-all shadow-2xs hover:border-slate-400"
              >
                <Bot className="w-4 h-4 text-indigo-600" />
                <span>AI Budget Curator</span>
              </button>
            </div>

            {/* Trust bullet checkpoints */}
            <div className="flex flex-wrap items-center gap-2.5 pt-2 text-xs font-bold text-slate-700">
              <div className="inline-flex items-center gap-1.5 bg-emerald-50/90 border border-emerald-200/90 text-emerald-900 px-3 py-1.5 rounded-lg shadow-2xs">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 shrink-0" />
                <span>Zero Middleman Markups</span>
              </div>
              <div className="inline-flex items-center gap-1.5 bg-emerald-50/90 border border-emerald-200/90 text-emerald-900 px-3 py-1.5 rounded-lg shadow-2xs">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 shrink-0" />
                <span>100% Free Insured Transit</span>
              </div>
              <div className="inline-flex items-center gap-1.5 bg-emerald-50/90 border border-emerald-200/90 text-emerald-900 px-3 py-1.5 rounded-lg shadow-2xs">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 shrink-0" />
                <span>PESO Certified Green Crackers</span>
              </div>
            </div>
          </div>

          {/* Right Column (5 cols): Dynamic Image Only - Comes & Goes */}
          <div className="md:col-span-5 flex items-center justify-center md:justify-end w-full">
            <div
              className="relative w-full max-w-[340px] sm:max-w-[380px] lg:max-w-[420px] aspect-[4/5] rounded-3xl overflow-hidden shadow-2xl shadow-slate-900/15 border border-slate-200/80 bg-slate-950 group"
              onMouseEnter={() => setIsPaused(true)}
              onMouseLeave={() => setIsPaused(false)}
            >
              {/* Subtle ambient festive glow */}
              <div className="absolute -inset-2 bg-gradient-to-tr from-amber-500/20 via-transparent to-emerald-500/20 blur-xl opacity-60 pointer-events-none" />

              <AnimatePresence mode="wait">
                <motion.div
                  key={currentPerson.id}
                  initial={{ opacity: 0, scale: 0.96 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.96 }}
                  transition={{ duration: 0.6, ease: [0.16, 1, 0.3, 1] }}
                  className="relative w-full h-full"
                >
                  <img
                    src={currentPerson.image}
                    alt={currentPerson.title}
                    className={`w-full h-full object-cover ${currentPerson.objectPosition}`}
                  />

                  {/* Gradient shadow for text legibility */}
                  <div className="absolute inset-0 bg-gradient-to-t from-slate-950/90 via-slate-950/30 to-transparent pointer-events-none" />

                  {/* Top Badge: Trusted by them */}
                  <div className="absolute top-4 left-4 inline-flex items-center bg-slate-950/80 backdrop-blur-md text-amber-300 text-xs font-black uppercase tracking-wider px-3.5 py-1.5 rounded-full border border-amber-500/30 shadow-lg">
                    <span>Trusted by them</span>
                  </div>

                  {/* Bottom Text Content */}
                  <div className="absolute bottom-5 inset-x-5 text-left text-white space-y-1.5">
                    <div className="inline-flex items-center gap-1.5 text-emerald-400 text-xs font-extrabold uppercase tracking-wider">
                      <ShieldCheck className="w-4 h-4 text-emerald-400 shrink-0" />
                      <span>Direct Sivakasi Hub</span>
                    </div>

                    <h3 className="text-xl sm:text-2xl font-black tracking-tight text-white leading-tight">
                      Trusted by them
                    </h3>

                    <p className="text-xs sm:text-[13px] text-slate-200 font-medium leading-relaxed">
                      {currentPerson.quote}
                    </p>
                  </div>
                </motion.div>
              </AnimatePresence>

              {/* Minimal Slide Indicators */}
              <div className="absolute top-4 right-4 z-10 flex items-center gap-1.5 bg-slate-950/60 backdrop-blur-md px-2.5 py-1.5 rounded-full border border-white/10">
                {TRUSTED_PERSONS.map((p, idx) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setCurrentIdx(idx)}
                    className={`h-1.5 rounded-full transition-all duration-300 ${
                      currentIdx === idx ? 'w-5 bg-amber-400' : 'w-1.5 bg-white/40 hover:bg-white/70'
                    }`}
                    aria-label={`Switch to image ${idx + 1}`}
                  />
                ))}
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
              100% Free Delivery
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
