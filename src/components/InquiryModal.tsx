import React, { useState } from 'react';
import { X, Building, Check } from 'lucide-react';
import { CrackerItem } from '../types';

interface InquiryModalProps {
  isOpen: boolean;
  onClose: () => void;
  item: CrackerItem | null;
}

export const InquiryModal: React.FC<InquiryModalProps> = ({
  isOpen,
  onClose,
  item,
}) => {
  const [submitted, setSubmitted] = useState(false);
  const [formData, setFormData] = useState({
    name: 'John Doe',
    email: 'company@email.com',
    phone: '1234567890',
    message: 'We are requesting bulk batch availability and custom festival delivery schedules for this item.',
  });

  if (!isOpen || !item) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    setTimeout(() => {
      setSubmitted(false);
      onClose();
    }, 2200);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs animate-in fade-in duration-200">
      <div
        className="w-full max-w-md bg-white rounded-3xl shadow-2xl border border-neutral-200 p-8 text-left animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between pb-4 border-b border-neutral-100">
          <div className="flex items-center gap-2">
            <Building className="w-5 h-5 text-[#373A40]" />
            <h2 className="text-base font-bold text-[#23262C]">
              Property / Batch Inquiry
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-full hover:bg-neutral-100 text-[#4E525C]"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {submitted ? (
          <div className="py-12 text-center space-y-3">
            <div className="w-12 h-12 rounded-full bg-emerald-50 text-emerald-600 flex items-center justify-center mx-auto">
              <Check className="w-6 h-6" />
            </div>
            <h4 className="text-base font-bold text-neutral-800">
              Inquiry Dispatched!
            </h4>
            <p className="text-xs text-neutral-500">
              Our Sivakasi concierge desk will respond within 2 business hours.
            </p>
          </div>
        ) : (
          <form onSubmit={handleSubmit} className="mt-5 space-y-4">
            <div className="p-3 bg-neutral-50 rounded-2xl border border-neutral-200">
              <p className="text-xs font-bold text-neutral-800">{item.title}</p>
              <p className="text-[11px] text-neutral-500">{item.location}</p>
            </div>

            <div className="space-y-1">
              <label className="text-[11px] font-bold text-[#3B3E45]">
                Full Name
              </label>
              <input
                type="text"
                required
                value={formData.name}
                onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                className="w-full text-xs font-semibold px-4 py-2.5 rounded-xl border border-neutral-300 outline-none"
              />
            </div>

            <div className="space-y-1">
              <label className="text-[11px] font-bold text-[#3B3E45]">
                Email Address
              </label>
              <input
                type="email"
                required
                value={formData.email}
                onChange={(e) => setFormData({ ...formData, email: e.target.value })}
                className="w-full text-xs font-semibold px-4 py-2.5 rounded-xl border border-neutral-300 outline-none"
              />
            </div>

            <div className="space-y-1">
              <label className="text-[11px] font-bold text-[#3B3E45]">
                Inquiry Details
              </label>
              <textarea
                rows={3}
                required
                value={formData.message}
                onChange={(e) => setFormData({ ...formData, message: e.target.value })}
                className="w-full text-xs font-semibold px-4 py-2.5 rounded-xl border border-neutral-300 outline-none resize-none"
              />
            </div>

            <button
              type="submit"
              className="w-full bg-[#373A40] hover:bg-[#25272B] text-white text-xs font-bold py-3.5 rounded-full shadow-sm mt-2 transition-all"
            >
              Send Inquiry
            </button>
          </form>
        )}
      </div>
    </div>
  );
};
