import React, { useState, useEffect } from 'react';
import { ArrowUp, Download, Check } from 'lucide-react';
import { supportWhatsAppUrl } from '../lib/contact';

export const FloatingActions: React.FC = () => {
  const [showScrollTop, setShowScrollTop] = useState(false);
  const [downloaded, setDownloaded] = useState(false);

  useEffect(() => {
    const handleScroll = () => {
      setShowScrollTop(window.scrollY > 300);
    };
    window.addEventListener('scroll', handleScroll);
    return () => window.removeEventListener('scroll', handleScroll);
  }, []);

  const scrollToTop = () => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const openWhatsApp = () => {
    window.open(supportWhatsAppUrl('Hello Zhi-mart Crackers, I want to order crackers!'), '_blank', 'noopener,noreferrer');
  };

  const handleDownload = () => {
    setDownloaded(true);
    setTimeout(() => {
      setDownloaded(false);
    }, 2000);
  };

  return (
    <div className="fixed bottom-6 right-4 sm:right-6 z-40 flex flex-col items-end space-y-3 pointer-events-none">
      {/* 2026 Price List Download Button — Crisp pill with text & icon */}
      <a
        href="/ZHI-MART_Price_List_85_Discount_2026_.docx"
        download="ZHI-MART_Price_List_85_Discount_2026_.docx"
        onClick={handleDownload}
        className="pointer-events-auto inline-flex items-center gap-2 px-4 py-2.5 sm:px-4.5 sm:py-2.5 rounded-full bg-blue-600 hover:bg-blue-700 text-white shadow-xl hover:shadow-2xl hover:scale-105 active:scale-95 transition-all font-extrabold text-xs sm:text-sm tracking-tight border border-blue-400/30"
        aria-label="Download Price List"
        title="Download 2026 Price List (85% Discount)"
      >
        {downloaded ? (
          <>
            <Check className="w-4 h-4 text-emerald-200 stroke-[3] shrink-0" />
            <span>Downloaded!</span>
          </>
        ) : (
          <>
            <Download className="w-4 h-4 text-white stroke-[2.5] shrink-0" />
            <span>Download Price List</span>
          </>
        )}
      </a>

      {/* WhatsApp & Scroll-to-Top stack (centered with each other) */}
      <div className="pointer-events-auto flex flex-col items-center space-y-3 mr-1 sm:mr-1.5">
        {/* WhatsApp Floating Button */}
        <button
          onClick={openWhatsApp}
          className="w-13 h-13 sm:w-14 sm:h-14 bg-[#25D366] hover:bg-[#20ba5a] text-white rounded-full flex items-center justify-center shadow-xl hover:scale-108 active:scale-95 transition-all p-3"
          aria-label="Chat on WhatsApp"
          title="WhatsApp Support"
        >
          <svg
            viewBox="0 0 24 24"
            width="28"
            height="28"
            stroke="currentColor"
            strokeWidth="2"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="text-white fill-white"
          >
            <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z" />
          </svg>
        </button>

        {/* Scroll to Top Button */}
        {showScrollTop && (
          <button
            onClick={scrollToTop}
            className="w-11 h-11 bg-[#0F172A] hover:bg-slate-800 text-white rounded-full flex items-center justify-center shadow-lg hover:scale-105 active:scale-95 transition-all"
            aria-label="Scroll to top"
          >
            <ArrowUp className="w-5 h-5" />
          </button>
        )}
      </div>
    </div>
  );
};
