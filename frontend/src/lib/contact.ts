import { BUSINESS_WHATSAPP_NUMBER } from './checkout';

/** Business phone / WhatsApp, derived from the single source of truth in checkout.ts. */
const LOCAL = BUSINESS_WHATSAPP_NUMBER.slice(-10);

/** `+91 95979 61800` */
export const SUPPORT_PHONE_DISPLAY = `+91 ${LOCAL.slice(0, 5)} ${LOCAL.slice(5)}`;
/** `+919597961800` (use as `tel:${SUPPORT_PHONE_TEL}`) */
export const SUPPORT_PHONE_TEL = `+${BUSINESS_WHATSAPP_NUMBER}`;

/** wa.me chat link to the business number with a prefilled message. */
export function supportWhatsAppUrl(message: string): string {
  return `https://wa.me/${BUSINESS_WHATSAPP_NUMBER}?text=${encodeURIComponent(message)}`;
}
