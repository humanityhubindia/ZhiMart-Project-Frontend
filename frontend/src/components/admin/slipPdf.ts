/**
 * Renders the on-screen order estimation slip (`#printable-estimation-slip`) to an A4 PDF `File`,
 * so it can be handed to the OS share sheet (→ WhatsApp) or downloaded. The PDF libraries are
 * loaded on demand, keeping them out of the storefront bundle.
 */

/** Width the slip is laid out at for the capture (desktop layout, whatever the admin's screen). */
const CAPTURE_WIDTH_PX = 800;

export async function slipPdfFile(slip: HTMLElement, fileName: string): Promise<File> {
  const [{ default: html2canvas }, { jsPDF }] = await Promise.all([import('html2canvas-pro'), import('jspdf')]);
  const canvas = await html2canvas(slip, {
    scale: 2,
    backgroundColor: '#ffffff',
    useCORS: true,
    logging: false,
    windowWidth: 1024,
    onclone: (doc) => {
      const el = doc.getElementById(slip.id);
      if (!el) return;
      el.style.width = `${CAPTURE_WIDTH_PX}px`;
      el.style.maxWidth = 'none';
      // The modal's scroll containers would otherwise clip a long slip.
      for (let p = el.parentElement; p; p = p.parentElement) {
        p.style.overflow = 'visible';
        p.style.maxHeight = 'none';
      }
    },
  });

  const pdf = new jsPDF({ unit: 'mm', format: 'a4', orientation: 'portrait', compress: true });
  const margin = 8;
  const pageW = pdf.internal.pageSize.getWidth() - margin * 2;
  const pageH = pdf.internal.pageSize.getHeight() - margin * 2;
  // Slice the capture into page-height strips so long orders run onto further pages.
  const sliceHeightPx = Math.floor((pageH / pageW) * canvas.width);
  for (let y = 0, page = 0; y < canvas.height; y += sliceHeightPx, page++) {
    const h = Math.min(sliceHeightPx, canvas.height - y);
    const strip = document.createElement('canvas');
    strip.width = canvas.width;
    strip.height = h;
    const ctx = strip.getContext('2d');
    if (!ctx) throw new Error('Canvas is not available');
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, strip.width, strip.height);
    ctx.drawImage(canvas, 0, y, canvas.width, h, 0, 0, canvas.width, h);
    if (page > 0) pdf.addPage();
    pdf.addImage(strip.toDataURL('image/jpeg', 0.92), 'JPEG', margin, margin, pageW, (h / canvas.width) * pageW);
  }
  return new File([pdf.output('blob')], fileName, { type: 'application/pdf' });
}

/** Can this browser put a PDF in the share sheet (mobile Chrome/Safari, Safari/Edge on desktop)? */
export function canShareFile(file: File): boolean {
  return typeof navigator !== 'undefined' && !!navigator.canShare && navigator.canShare({ files: [file] });
}

export function downloadFile(file: File) {
  const url = URL.createObjectURL(file);
  const a = document.createElement('a');
  a.href = url;
  a.download = file.name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
