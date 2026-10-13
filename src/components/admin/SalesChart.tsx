import React, { useId, useState } from 'react';
import { formatCount, formatDayLabel, formatINR, formatINRCompact } from './format';

type Metric = 'revenue' | 'orders';

interface Day {
  date: string;
  orders: number;
  revenue: number;
}

/** "Nice" axis max: 1/2/2.5/5 × 10^n ≥ value. */
function niceMax(value: number): number {
  if (value <= 0) return 1;
  const exp = Math.pow(10, Math.floor(Math.log10(value)));
  for (const step of [1, 2, 2.5, 5, 10]) {
    if (step * exp >= value) return step * exp;
  }
  return 10 * exp;
}

/**
 * 14-day single-series bar chart (CSS only, no chart dependency).
 * Every bar is focusable with a tooltip; a data table is available for screen readers.
 */
export function SalesChart({ days }: { days: Day[] }) {
  const [metric, setMetric] = useState<Metric>('revenue');
  const [active, setActive] = useState<number | null>(null);
  const tableId = useId();

  const values = days.map((d) => (metric === 'revenue' ? d.revenue : d.orders));
  const max = niceMax(Math.max(0, ...values));
  const ticks = [max, max / 2, 0];
  const fmt = (v: number) => (metric === 'revenue' ? formatINR(v) : formatCount(v));
  const fmtAxis = (v: number) => (metric === 'revenue' ? formatINRCompact(v) : formatCount(Math.round(v)));
  const total = values.reduce((a, b) => a + b, 0);
  const peakIdx = values.length ? values.indexOf(Math.max(...values)) : -1;
  const lastIdx = values.length - 1;

  return (
    <figure className="bg-white p-5 sm:p-6 rounded-3xl border border-slate-200/90 shadow-2xs">
      <div className="flex flex-wrap items-start justify-between gap-3 mb-5">
        <figcaption>
          <h3 className="text-base font-extrabold text-slate-950">
            {metric === 'revenue' ? 'Sales revenue' : 'Orders placed'} · last 14 days
          </h3>
          <p className="text-xs text-slate-500 mt-0.5">
            IST calendar days, cancelled orders and refunds excluded. Total{' '}
            <span className="font-extrabold text-slate-800 tabular-nums">{fmt(total)}</span>
          </p>
        </figcaption>
        <div role="group" aria-label="Chart metric" className="inline-flex bg-slate-100 rounded-xl p-0.5">
          {(['revenue', 'orders'] as Metric[]).map((m) => (
            <button
              key={m}
              type="button"
              aria-pressed={metric === m}
              onClick={() => setMetric(m)}
              className={`px-3 py-1.5 rounded-[10px] text-xs font-bold transition-all ${
                metric === m ? 'bg-white text-slate-950 shadow-xs' : 'text-slate-500 hover:text-slate-800'
              }`}
            >
              {m === 'revenue' ? 'Revenue' : 'Orders'}
            </button>
          ))}
        </div>
      </div>

      {days.length === 0 ? (
        <p className="text-xs text-slate-500 py-10 text-center">No sales data yet.</p>
      ) : (
        <div className="flex gap-2" aria-describedby={tableId}>
          {/* Y axis */}
          <div className="relative w-12 shrink-0 h-48 text-[10px] font-semibold text-slate-400 tabular-nums" aria-hidden>
            {ticks.map((t, i) => (
              <span
                key={i}
                className="absolute right-0 -translate-y-1/2"
                style={{ top: `${(1 - t / max) * 100}%` }}
              >
                {fmtAxis(t)}
              </span>
            ))}
          </div>

          <div className="flex-1 min-w-0">
            {/* Plot */}
            <div className="relative h-48">
              {ticks.map((t, i) => (
                <div
                  key={i}
                  aria-hidden
                  className={`absolute left-0 right-0 border-t ${t === 0 ? 'border-slate-300' : 'border-dashed border-slate-200'}`}
                  style={{ top: `${(1 - t / max) * 100}%` }}
                />
              ))}
              <div className="absolute inset-0 flex items-end gap-[2px]">
                {days.map((d, i) => {
                  const v = values[i];
                  const pct = max > 0 ? (v / max) * 100 : 0;
                  const label = formatDayLabel(d.date);
                  const showLabel = v > 0 && (i === peakIdx || i === lastIdx);
                  const isActive = active === i;
                  return (
                    <div
                      key={d.date}
                      className="relative flex-1 h-full flex items-end justify-center group"
                      onMouseEnter={() => setActive(i)}
                      onMouseLeave={() => setActive((cur) => (cur === i ? null : cur))}
                    >
                      <button
                        type="button"
                        className="absolute inset-0 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500 rounded-md"
                        aria-label={`${label.weekday} ${label.day}: ${formatINR(d.revenue)} revenue, ${formatCount(d.orders)} orders`}
                        onFocus={() => setActive(i)}
                        onBlur={() => setActive((cur) => (cur === i ? null : cur))}
                      />
                      {showLabel && !isActive && (
                        <span
                          aria-hidden
                          className="absolute text-[10px] font-extrabold text-slate-700 tabular-nums whitespace-nowrap -translate-y-1"
                          style={{ bottom: `${pct}%` }}
                        >
                          {fmtAxis(v)}
                        </span>
                      )}
                      <div
                        aria-hidden
                        className={`w-full max-w-[28px] rounded-t-[4px] transition-colors ${
                          v === 0 ? 'bg-slate-200' : isActive ? 'bg-amber-600' : 'bg-amber-500'
                        }`}
                        style={{ height: v === 0 ? '2px' : `max(${pct}%, 3px)` }}
                      />
                      {isActive && (
                        <div
                          role="tooltip"
                          className={`absolute z-10 pointer-events-none bg-slate-950 text-white rounded-xl px-3 py-2 text-[11px] font-bold shadow-xl whitespace-nowrap ${
                            i < 3 ? 'left-0' : i > days.length - 4 ? 'right-0' : 'left-1/2 -translate-x-1/2'
                          }`}
                          style={{ bottom: `calc(${pct}% + 6px)` }}
                        >
                          <p className="text-slate-300 font-semibold">
                            {label.weekday}, {label.day}
                          </p>
                          <p className="tabular-nums">{formatINR(d.revenue)} revenue</p>
                          <p className="tabular-nums text-slate-300">
                            {formatCount(d.orders)} order{d.orders === 1 ? '' : 's'}
                          </p>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>

            {/* X axis */}
            <div className="flex gap-[2px] mt-2" aria-hidden>
              {days.map((d, i) => {
                const label = formatDayLabel(d.date);
                const show = i % 2 === lastIdx % 2;
                return (
                  <div key={d.date} className="flex-1 text-center text-[10px] font-semibold text-slate-500 leading-tight">
                    {show ? (
                      <>
                        <span className="block">{label.day}</span>
                        <span className="block text-slate-400">{label.weekday}</span>
                      </>
                    ) : (
                      <span className="block">&nbsp;</span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}

      <table id={tableId} className="sr-only">
        <caption>Daily sales for the last 14 days</caption>
        <thead>
          <tr>
            <th scope="col">Date</th>
            <th scope="col">Orders</th>
            <th scope="col">Revenue</th>
          </tr>
        </thead>
        <tbody>
          {days.map((d) => (
            <tr key={d.date}>
              <th scope="row">{d.date}</th>
              <td>{d.orders}</td>
              <td>{formatINR(d.revenue)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}
