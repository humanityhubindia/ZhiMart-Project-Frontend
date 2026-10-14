import React, { useState } from 'react';
import { Send, Flame } from 'lucide-react';
import {
  BUSINESS_ADDRESS_LINES,
  BUSINESS_EMAIL,
  SUPPORT_PHONE_DISPLAY,
  SUPPORT_PHONE_TEL,
  supportWhatsAppUrl,
} from '../lib/contact';

export const Footer: React.FC = () => {
  const [email, setEmail] = useState('');
  const [subscribed, setSubscribed] = useState(false);

  const handleSubscribe = (e: React.FormEvent) => {
    e.preventDefault();
    if (email.trim()) {
      setSubscribed(true);
      setTimeout(() => setSubscribed(false), 3000);
      setEmail('');
    }
  };

  return (
    <footer className="w-full bg-[#EAECEF] text-[#33363D] pt-12 pb-8 border-t border-[#DDE1E6] text-left">
      <div className="max-w-7xl mx-auto px-6">
        {/* Top Newsletter Bar matching Screenshot 3 */}
        <div className="bg-[#DFE3E7] rounded-3xl p-6 md:p-8 flex flex-col md:flex-row items-center justify-between gap-6 mb-16">
          <div className="w-full md:w-auto">
            <h3 className="text-base font-black text-[#26282E] tracking-tight uppercase">
              NEWSLETTER
            </h3>
            <p className="text-xs text-[#7B808C] font-semibold mt-0.5">
              Stay Upto Date With Diwali 2026 Price Drops
            </p>
          </div>

          <form
            onSubmit={handleSubscribe}
            className="w-full md:max-w-xl bg-white rounded-full p-1.5 pl-6 flex items-center justify-between shadow-xs border border-neutral-200"
          >
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="Your Email for instant price catalog..."
              required
              className="w-full text-xs text-[#454850] placeholder:text-[#9EA2AC] bg-transparent outline-none pr-3"
            />
            <button
              type="submit"
              className="w-10 h-10 rounded-full bg-[#828891] hover:bg-[#686E78] text-white flex items-center justify-center shrink-0 transition-transform active:scale-95 shadow-xs"
              aria-label="Submit Email"
            >
              <Send className="w-4 h-4 ml-0.5" />
            </button>
          </form>
        </div>

        {subscribed && (
          <div className="text-center text-xs font-semibold text-emerald-700 -mt-10 mb-8">
            Thank you! Zhi-mart 2026 Diwali rate card has been sent to your email.
          </div>
        )}

        {/* Main Footer Columns */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-10 pb-16 border-b border-[#D5DAE0]">
          {/* Brand Info */}
          <div className="lg:col-span-2 space-y-4 pr-4">
            <div className="flex items-center space-x-2.5">
              <div className="w-8 h-8 rounded-lg bg-gradient-to-tr from-amber-500 to-red-500 flex items-center justify-center">
                <Flame className="w-4 h-4 text-white fill-white" />
              </div>
              <h2 className="text-2xl font-black text-[#24262B] tracking-tight font-heading">
                Zhi-mart Crackers
              </h2>
            </div>
            <p className="text-xs text-[#6B707B] leading-relaxed max-w-sm">
              Direct factory fireworks from Sivakasi, Tamil Nadu. 100% genuine green crackers, safe Visco delay fuses, and verified PESO licensed distribution.
            </p>
          </div>

          {/* COMPANY column */}
          <div className="space-y-4">
            <h4 className="text-xs font-black text-[#23252A] tracking-wider uppercase">
              COMPANY
            </h4>
            <ul className="space-y-2.5 text-xs font-medium text-[#585C66]">
              <li><a href="#about" className="hover:text-black">About Zhi-mart</a></li>
              <li><a href="#legal" className="hover:text-black">Sivakasi License</a></li>
              <li><a href="#safety" className="hover:text-black">Safety Compliance</a></li>
              <li>
                <a
                  href="?page=admin"
                  className="text-amber-800 hover:text-amber-950 font-extrabold inline-flex items-center gap-1 bg-amber-100/60 hover:bg-amber-100 px-2 py-0.5 rounded"
                >
                  Admin Portal (Sivakasi)
                </a>
              </li>
            </ul>
          </div>

          {/* HELP CENTER column */}
          <div className="space-y-4">
            <h4 className="text-xs font-black text-[#23252A] tracking-wider uppercase">
              HELP CENTER
            </h4>
            <ul className="space-y-2.5 text-xs font-medium text-[#585C66]">
              <li><a href="#track" className="hover:text-black">Track Parcel Docket</a></li>
              <li><a href="#how" className="hover:text-black">How To Order?</a></li>
              <li><a href="#estimate" className="hover:text-black">Quick Estimate Guide</a></li>
              <li><a href="#faqs" className="hover:text-black">FAQs</a></li>
            </ul>
          </div>

          {/* CONTACT INFO column */}
          <div className="space-y-4">
            <h4 className="text-xs font-black text-[#23252A] tracking-wider uppercase">
              CONTACT INFO
            </h4>
            <div className="space-y-2 text-xs font-medium text-[#585C66]">
              <p>
                Helpline:{' '}
                <a href={`tel:${SUPPORT_PHONE_TEL}`} className="hover:text-black">
                  {SUPPORT_PHONE_DISPLAY}
                </a>
              </p>
              <p>
                WhatsApp:{' '}
                <a
                  href={supportWhatsAppUrl('Hello Zhi-mart Crackers, I want to order crackers!')}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="hover:text-black"
                >
                  {SUPPORT_PHONE_DISPLAY}
                </a>
              </p>
              <p>
                Email:{' '}
                <a href={`mailto:${BUSINESS_EMAIL}`} className="hover:text-black transition-colors font-medium">
                  {BUSINESS_EMAIL}
                </a>
              </p>
              <address className="not-italic leading-relaxed">
                Address:
                {BUSINESS_ADDRESS_LINES.map((line) => (
                  <span key={line} className="block">
                    {line}
                  </span>
                ))}
              </address>
            </div>
          </div>
        </div>

        {/* Bottom Copyright Row */}
        <div className="pt-8 flex flex-col sm:flex-row items-center justify-between text-xs font-semibold text-[#666B75] gap-3">
          <p>© 2026 Zhi-mart Crackers | All rights reserved</p>
          <div className="flex items-center gap-3">
            <span>Handcrafted for Premium Celebrations</span>
            <span>•</span>
            <a href="?page=admin" className="text-slate-800 hover:text-black font-extrabold underline">
              Admin Login
            </a>
          </div>
        </div>
      </div>
    </footer>
  );
};
