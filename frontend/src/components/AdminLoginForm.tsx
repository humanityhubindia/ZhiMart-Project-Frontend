import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Mail,
  Lock,
  Eye,
  EyeOff,
  ShieldCheck,
  AlertCircle,
  ArrowLeft,
  Loader2,
  RotateCw,
  CheckCircle2,
} from 'lucide-react';
import { useAdminAuth } from '../auth/AdminAuthContext';
import { ApiError, AuthSession, isApiError } from '../lib/api';

interface AdminLoginFormProps {
  /** Smaller typography/padding for use inside the AuthModal. */
  compact?: boolean;
  onSuccess?: (session: AuthSession) => void;
}

const OTP_LENGTH = 6;
const EMPTY_OTP = Array<string>(OTP_LENGTH).fill('');

type Step = 'credentials' | 'otp';

interface FormError {
  message: string;
  /** When set, a live "try again in m:ss" countdown is appended and submit is disabled. */
  until?: number;
}

interface Challenge {
  challengeId: string;
  maskedEmail: string;
  expiresAt: number;
  resendAvailableAt: number;
}

/** Re-renders every second while `active`, returning the current timestamp. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [active]);
  return now;
}

function secondsLeft(until: number | undefined, now: number): number {
  return until ? Math.max(0, Math.ceil((until - now) / 1000)) : 0;
}

function formatCountdown(totalSeconds: number): string {
  const s = Math.max(0, totalSeconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = h > 0 ? String(m).padStart(2, '0') : String(m);
  return `${h > 0 ? `${h}:` : ''}${mm}:${String(sec).padStart(2, '0')}`;
}

const untilFrom = (retryAfter?: number) =>
  retryAfter && retryAfter > 0 ? Date.now() + retryAfter * 1000 : undefined;

/** Shared mapping for errors that look the same on both steps. */
function genericError(err: unknown): FormError {
  if (!isApiError(err)) return { message: 'Something went wrong. Please try again.' };
  switch (err.code) {
    case 'account_locked':
      return {
        message: 'Too many failed attempts. This account is temporarily locked.',
        until: untilFrom(err.retryAfter),
      };
    case 'too_many_requests':
      return {
        message: 'Too many attempts. Please wait before trying again.',
        until: untilFrom(err.retryAfter),
      };
    case 'network_error':
    case 'timeout':
      return { message: "Can't reach the server. Check your connection and try again." };
    case 'service_unavailable':
      return { message: 'The admin service is temporarily unavailable. Please try again in a moment.' };
    case 'forbidden':
      return { message: 'This request was blocked by a security check. Please reload the page and try again.' };
    default:
      return { message: err.message || 'Something went wrong. Please try again.' };
  }
}

function credentialsError(err: unknown): FormError {
  if (isApiError(err)) {
    if (err.code === 'invalid_credentials') return { message: 'Invalid email or password.' };
    if (err.code === 'validation_error') return { message: 'Please enter a valid email address and password.' };
    // Per-(email, IP) sign-in throttling.
    if (err.code === 'too_many_requests') {
      return {
        message: 'Too many sign-in attempts. Please wait before trying again.',
        until: untilFrom(err.retryAfter),
      };
    }
  }
  return genericError(err);
}

export const AdminLoginForm: React.FC<AdminLoginFormProps> = ({ compact = false, onSuccess }) => {
  const { login, verifyOtp, resendOtp } = useAdminAuth();

  const [step, setStep] = useState<Step>('credentials');

  // Step 1
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Step 2
  const [challenge, setChallenge] = useState<Challenge | null>(null);
  const [digits, setDigits] = useState<string[]>(EMPTY_OTP);
  const [verifying, setVerifying] = useState(false);
  const [resending, setResending] = useState(false);
  const [info, setInfo] = useState<string | null>(null);

  const [error, setError] = useState<FormError | null>(null);

  const emailRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const otpRefs = useRef<Array<HTMLInputElement | null>>([]);
  /** Last code auto-submitted, so a failed (e.g. network) attempt isn't auto-retried in a loop. */
  const lastAutoSubmitted = useRef<string | null>(null);
  const busyRef = useRef(false);

  const tickActive = !!error?.until || step === 'otp';
  const now = useNow(tickActive);

  const errorSecondsLeft = secondsLeft(error?.until, now);
  const isCoolingDown = errorSecondsLeft > 0;
  const otpSecondsLeft = challenge ? secondsLeft(challenge.expiresAt, now) : 0;
  const resendSecondsLeft = challenge ? secondsLeft(challenge.resendAvailableAt, now) : 0;
  const code = digits.join('');

  // When a lockout/rate-limit countdown finishes, clear the message.
  useEffect(() => {
    if (error?.until && errorSecondsLeft === 0) setError(null);
  }, [error, errorSecondsLeft]);

  const backToCredentials = useCallback(
    (nextError: FormError | null, focus: 'email' | 'password' = 'password') => {
      setStep('credentials');
      setChallenge(null);
      setDigits(EMPTY_OTP);
      setInfo(null);
      setError(nextError);
      lastAutoSubmitted.current = null;
      setTimeout(() => {
        const el = focus === 'email' ? emailRef.current : passwordRef.current;
        el?.focus();
        if (focus === 'email') el?.select();
      }, 0);
    },
    []
  );

  // OTP expired locally → restart sign-in.
  useEffect(() => {
    if (step === 'otp' && challenge && otpSecondsLeft === 0 && !verifying) {
      backToCredentials({ message: 'Code expired — please sign in again.' });
    }
  }, [step, challenge, otpSecondsLeft, verifying, backToCredentials]);

  // Focus management on step change.
  useEffect(() => {
    if (step === 'otp') otpRefs.current[0]?.focus();
    else if (!email) emailRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step]);

  // ---- Step 1: credentials ----
  const handleCredentialsSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busyRef.current || isCoolingDown) return;
    busyRef.current = true;
    setSubmitting(true);
    setError(null);
    try {
      const res = await login(email, password);
      const t = Date.now();
      setChallenge({
        challengeId: res.challengeId,
        maskedEmail: res.maskedEmail,
        expiresAt: t + res.expiresIn * 1000,
        resendAvailableAt: t + res.resendAvailableIn * 1000,
      });
      setPassword(''); // never keep the password around once it's been used
      setShowPassword(false);
      setDigits(EMPTY_OTP);
      setInfo(null);
      lastAutoSubmitted.current = null;
      setStep('otp');
    } catch (err) {
      setError(credentialsError(err));
    } finally {
      busyRef.current = false;
      setSubmitting(false);
    }
  };

  // ---- Step 2: OTP ----
  const submitCode = useCallback(
    async (otp: string) => {
      if (!challenge || busyRef.current || otp.length !== OTP_LENGTH) return;
      busyRef.current = true;
      setVerifying(true);
      setError(null);
      setInfo(null);
      try {
        const session = await verifyOtp(challenge.challengeId, otp);
        onSuccess?.(session);
      } catch (err) {
        const apiErr = isApiError(err) ? err : null;
        if (apiErr?.code === 'otp_invalid') {
          const n = apiErr.attemptsRemaining;
          setError({
            message:
              typeof n === 'number'
                ? `Incorrect code. ${n} ${n === 1 ? 'attempt' : 'attempts'} left.`
                : 'Incorrect code. Please try again.',
          });
          setDigits(EMPTY_OTP);
          lastAutoSubmitted.current = null;
          setTimeout(() => otpRefs.current[0]?.focus(), 0);
        } else if (apiErr?.code === 'otp_challenge_expired') {
          backToCredentials({ message: 'Code expired — please sign in again.' });
        } else if (apiErr?.code === 'account_locked') {
          backToCredentials(genericError(apiErr));
        } else {
          // Network / 5xx / rate limit: keep the digits so the user can retry.
          setError(genericError(err as ApiError));
        }
      } finally {
        busyRef.current = false;
        setVerifying(false);
      }
    },
    [challenge, verifyOtp, onSuccess, backToCredentials]
  );

  // Auto-submit once all six digits are present.
  useEffect(() => {
    if (step !== 'otp' || code.length !== OTP_LENGTH || verifying || isCoolingDown) return;
    if (lastAutoSubmitted.current === code) return;
    lastAutoSubmitted.current = code;
    void submitCode(code);
  }, [step, code, verifying, isCoolingDown, submitCode]);

  const fillDigitsFrom = (startIndex: number, raw: string) => {
    const incoming = raw.replace(/\D/g, '');
    if (!incoming) return;
    // A full code always fills from the first box (paste / SMS-email autofill).
    const start = incoming.length >= OTP_LENGTH ? 0 : startIndex;
    const next = [...digits];
    for (let i = 0; i < incoming.length && start + i < OTP_LENGTH; i++) {
      next[start + i] = incoming[i];
    }
    setDigits(next);
    if (error && !error.until) setError(null);
    const focusIndex = Math.min(start + incoming.length, OTP_LENGTH - 1);
    otpRefs.current[focusIndex]?.focus();
  };

  const handleDigitChange = (index: number, e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    if (value === '') {
      const next = [...digits];
      next[index] = '';
      setDigits(next);
      return;
    }
    fillDigitsFrom(index, value);
  };

  const handleDigitKeyDown = (index: number, e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace') {
      if (digits[index]) {
        const next = [...digits];
        next[index] = '';
        setDigits(next);
      } else if (index > 0) {
        const next = [...digits];
        next[index - 1] = '';
        setDigits(next);
        otpRefs.current[index - 1]?.focus();
      }
      e.preventDefault();
    } else if (e.key === 'ArrowLeft' && index > 0) {
      otpRefs.current[index - 1]?.focus();
      e.preventDefault();
    } else if (e.key === 'ArrowRight' && index < OTP_LENGTH - 1) {
      otpRefs.current[index + 1]?.focus();
      e.preventDefault();
    }
  };

  const handleDigitPaste = (index: number, e: React.ClipboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    fillDigitsFrom(index, e.clipboardData.getData('text'));
  };

  const handleOtpSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    lastAutoSubmitted.current = code;
    void submitCode(code);
  };

  const handleResend = async () => {
    if (!challenge || resending || resendSecondsLeft > 0) return;
    setResending(true);
    setError(null);
    setInfo(null);
    try {
      const res = await resendOtp(challenge.challengeId);
      const t = Date.now();
      setChallenge({
        ...challenge,
        expiresAt: t + res.expiresIn * 1000,
        resendAvailableAt: t + res.resendAvailableIn * 1000,
      });
      setDigits(EMPTY_OTP);
      lastAutoSubmitted.current = null;
      setInfo(`A new code was sent to ${challenge.maskedEmail}.`);
      otpRefs.current[0]?.focus();
    } catch (err) {
      if (isApiError(err) && err.code === 'otp_challenge_expired') {
        backToCredentials({ message: 'Code expired — please sign in again.' });
      } else if (isApiError(err) && err.code === 'too_many_requests' && err.retryAfter) {
        // Cooldown: push the resend timer out instead of blocking verification.
        setChallenge({ ...challenge, resendAvailableAt: Date.now() + err.retryAfter * 1000 });
        setError({ message: 'Please wait before requesting another code.' });
      } else {
        setError(genericError(err));
      }
    } finally {
      setResending(false);
    }
  };

  // ---- Styling (compact = AuthModal, full = AdminPage) ----
  const inputClass = compact
    ? 'py-2.5 text-xs font-bold'
    : 'py-3 text-sm font-semibold focus:bg-white transition-all';
  const submitClass = compact
    ? 'py-3 font-black text-xs'
    : 'py-3.5 font-extrabold text-sm mt-2';
  const errorBoxClass = compact ? 'mb-5 p-3' : 'mb-6 p-3.5';
  const otpBoxClass = compact ? 'h-11 text-lg' : 'h-12 sm:h-14 text-xl';

  const errorText = error
    ? error.until && errorSecondsLeft > 0
      ? `${error.message} Try again in ${formatCountdown(errorSecondsLeft)}.`
      : error.message
    : '';

  const messages = (
    <div aria-live="polite" aria-atomic="true">
      {error && (
        <div
          role="alert"
          className={`${errorBoxClass} rounded-xl bg-rose-50 border border-rose-200 text-rose-700 text-xs font-bold flex items-center gap-2`}
        >
          <AlertCircle className="w-4 h-4 shrink-0" />
          <span>{errorText}</span>
        </div>
      )}
      {!error && info && (
        <div
          className={`${errorBoxClass} rounded-xl bg-emerald-50 border border-emerald-200 text-emerald-800 text-xs font-bold flex items-center gap-2`}
        >
          <CheckCircle2 className="w-4 h-4 shrink-0" />
          <span>{info}</span>
        </div>
      )}
    </div>
  );

  if (step === 'otp' && challenge) {
    const verifyDisabled = verifying || code.length !== OTP_LENGTH || isCoolingDown;
    return (
      <div>
        {messages}
        <form onSubmit={handleOtpSubmit} className="space-y-4" noValidate>
          <div>
            <p id="zm-otp-label" className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5">
              Verification Code
            </p>
            <p className="text-xs text-slate-500 mb-3">
              We sent a 6-digit code to{' '}
              <span className="font-extrabold text-slate-900">{challenge.maskedEmail}</span>.
            </p>
            <div
              role="group"
              aria-labelledby="zm-otp-label"
              className="grid grid-cols-6 gap-2"
            >
              {digits.map((d, i) => (
                <input
                  key={i}
                  ref={(el) => {
                    otpRefs.current[i] = el;
                  }}
                  type="text"
                  inputMode="numeric"
                  pattern="[0-9]*"
                  autoComplete={i === 0 ? 'one-time-code' : 'off'}
                  maxLength={OTP_LENGTH}
                  value={d}
                  disabled={verifying}
                  aria-label={`Digit ${i + 1} of ${OTP_LENGTH}`}
                  onChange={(e) => handleDigitChange(i, e)}
                  onKeyDown={(e) => handleDigitKeyDown(i, e)}
                  onPaste={(e) => handleDigitPaste(i, e)}
                  onFocus={(e) => e.target.select()}
                  className={`w-full ${otpBoxClass} text-center font-black text-slate-900 bg-slate-50 border border-slate-200 rounded-xl focus:outline-none focus:ring-2 focus:ring-amber-500 focus:bg-white transition-all disabled:opacity-60`}
                />
              ))}
            </div>
            <p className="mt-2 text-[11px] font-bold text-slate-500" aria-live="off">
              Code expires in{' '}
              <span className={otpSecondsLeft <= 30 ? 'text-rose-600' : 'text-amber-600'}>
                {formatCountdown(otpSecondsLeft)}
              </span>
            </p>
          </div>

          <button
            type="submit"
            disabled={verifyDisabled}
            className={`w-full ${submitClass} px-4 bg-slate-950 hover:bg-slate-900 text-white rounded-xl transition-all shadow-md active:scale-98 flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100`}
          >
            {verifying ? (
              <Loader2 className="w-4 h-4 text-amber-400 animate-spin" />
            ) : (
              <ShieldCheck className="w-4 h-4 text-amber-400" />
            )}
            <span>{verifying ? 'Verifying…' : 'Verify & Sign In'}</span>
          </button>
        </form>

        <div className="mt-4 flex items-center justify-between gap-2 text-xs font-bold">
          <button
            type="button"
            onClick={() => backToCredentials(null, 'email')}
            disabled={verifying}
            className="inline-flex items-center gap-1 text-slate-500 hover:text-slate-900 transition-colors disabled:opacity-60"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
            <span>Use a different account</span>
          </button>
          <button
            type="button"
            onClick={handleResend}
            disabled={resending || verifying || resendSecondsLeft > 0}
            className="inline-flex items-center gap-1 text-amber-700 hover:text-amber-900 transition-colors disabled:text-slate-400 disabled:cursor-not-allowed"
          >
            {resending ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <RotateCw className="w-3.5 h-3.5" />
            )}
            <span>
              {resendSecondsLeft > 0
                ? `Resend code in ${formatCountdown(resendSecondsLeft)}`
                : resending
                  ? 'Sending…'
                  : 'Resend code'}
            </span>
          </button>
        </div>
      </div>
    );
  }

  const signInDisabled = submitting || isCoolingDown;
  return (
    <div>
      {messages}
      <form onSubmit={handleCredentialsSubmit} className="space-y-4">
        <div>
          <label
            htmlFor={compact ? 'zm-admin-email-compact' : 'zm-admin-email'}
            className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5"
          >
            Admin Email ID
          </label>
          <div className="relative">
            <Mail className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
            <input
              ref={emailRef}
              id={compact ? 'zm-admin-email-compact' : 'zm-admin-email'}
              type="email"
              name="email"
              autoComplete="username"
              autoCapitalize="none"
              spellCheck={false}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="name@zhimart.com"
              required
              maxLength={254}
              disabled={submitting}
              className={`w-full pl-10 pr-4 ${inputClass} bg-slate-50 border border-slate-200 rounded-xl text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500 disabled:opacity-60`}
            />
          </div>
        </div>

        <div>
          <label
            htmlFor={compact ? 'zm-admin-password-compact' : 'zm-admin-password'}
            className="block text-xs font-bold text-slate-700 uppercase tracking-wider mb-1.5"
          >
            {compact ? 'Admin Password' : 'Admin Access Key'}
          </label>
          <div className="relative">
            <Lock className="w-4 h-4 text-slate-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
            <input
              ref={passwordRef}
              id={compact ? 'zm-admin-password-compact' : 'zm-admin-password'}
              type={showPassword ? 'text' : 'password'}
              name="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••••"
              required
              maxLength={256}
              disabled={submitting}
              className={`w-full pl-10 pr-10 ${inputClass} bg-slate-50 border border-slate-200 rounded-xl text-slate-900 focus:outline-none focus:ring-2 focus:ring-amber-500 disabled:opacity-60`}
            />
            <button
              type="button"
              onClick={() => setShowPassword((v) => !v)}
              aria-label={showPassword ? 'Hide password' : 'Show password'}
              aria-pressed={showPassword}
              className="absolute right-3.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600"
            >
              {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
            </button>
          </div>
        </div>

        <button
          type="submit"
          disabled={signInDisabled}
          className={`w-full ${submitClass} px-4 bg-slate-950 hover:bg-slate-900 text-white rounded-xl transition-all shadow-md active:scale-98 flex items-center justify-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100`}
        >
          {submitting ? (
            <Loader2 className="w-4 h-4 text-amber-400 animate-spin" />
          ) : (
            <ShieldCheck className="w-4 h-4 text-amber-400" />
          )}
          <span>
            {submitting
              ? 'Checking credentials…'
              : compact
                ? 'Sign In to Admin Console'
                : 'Authorize Admin Login'}
          </span>
        </button>
      </form>
    </div>
  );
};
