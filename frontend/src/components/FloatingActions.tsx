import React, { useState, useEffect } from 'react';
import { ArrowUp, PhoneCall, MessageCircle } from 'lucide-react';

export const FloatingActions: React.FC = () => {
  const [showScrollTop, setShowScrollTop] = useState(false);

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
    window.open('https://wa.me/919840012345?text=Hello%20Zhi-mart%20Crackers,%20I%20want%20to%20order%20crackers!', '_blank');
  };

  return (
    <div className="fixed bottom-6 right-6 z-40 flex flex-col items-center space-y-3">
      {/* WhatsApp Floating Button matching Screenshot 1 & 2 */}
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

      {/* Scroll to Top Button matching Screenshot 1 & 2 */}
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
  );
};
