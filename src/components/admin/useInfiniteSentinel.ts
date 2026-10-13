import { useEffect, useRef } from 'react';

/** Calls `onVisible` when the returned ref's element scrolls into view (infinite scroll). */
export function useInfiniteSentinel(enabled: boolean, onVisible: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const cb = useRef(onVisible);
  cb.current = onVisible;
  useEffect(() => {
    const el = ref.current;
    if (!el || !enabled || typeof IntersectionObserver === 'undefined') return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) cb.current();
      },
      { rootMargin: '400px 0px' }
    );
    io.observe(el);
    return () => io.disconnect();
  }, [enabled]);
  return ref;
}
