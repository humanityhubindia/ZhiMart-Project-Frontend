import React from 'react';
import { FlaskConical } from 'lucide-react';
import { useTestMode } from '../hooks/useProducts';

/** Sitewide notice while the server runs in ₹1 payment test mode. */
export const TestModeBanner: React.FC = () => {
  const testMode = useTestMode();
  if (!testMode) return null;
  return (
    <div
      role="status"
      className="w-full bg-amber-400 text-amber-950 border-b border-amber-500/60 px-4 py-1.5 text-center text-[11px] sm:text-xs font-bold"
    >
      <span className="inline-flex items-center gap-1.5">
        <FlaskConical className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
        <span className="font-black tracking-wide">TEST MODE</span>
        <span aria-hidden="true">—</span>
        <span>all products ₹1. Orders placed now are test orders.</span>
      </span>
    </div>
  );
};

/** Small tag shown instead of discount badges while in test mode. */
export const TestPriceTag: React.FC<{ className?: string }> = ({ className = '' }) => (
  <span
    className={`inline-flex items-center bg-amber-100 text-amber-900 border border-amber-300 text-[11px] font-black tracking-wide uppercase px-2.5 py-1 rounded-md ${className}`}
  >
    Test price
  </span>
);
