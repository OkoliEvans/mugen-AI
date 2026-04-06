import axios, { AxiosInstance, AxiosError } from 'axios';
import { VeilError } from './errors';

const DEFAULT_MAX_RETRIES = 3;
const RETRY_DELAY_MS = 500;

/**
 * Creates a pre-configured Axios instance with exponential backoff retry.
 */
export function createHttpClient(baseURL: string, maxRetries: number = DEFAULT_MAX_RETRIES): AxiosInstance {
  const client = axios.create({
    baseURL,
    headers: { 'Content-Type': 'application/json' },
    timeout: 30_000,
  });

  client.interceptors.response.use(
    (response) => response,
    async (error: AxiosError) => {
      const config = error.config as (typeof error.config) & { _retryCount?: number };
      if (!config) return Promise.reject(error);

      config._retryCount = config._retryCount ?? 0;

      // Only retry on network errors or 5xx responses
      const isRetryable =
        !error.response || (error.response.status >= 500 && error.response.status < 600);

      if (isRetryable && config._retryCount < maxRetries) {
        config._retryCount += 1;
        const delay = RETRY_DELAY_MS * Math.pow(2, config._retryCount - 1);
        await sleep(delay);
        return client(config);
      }

      return Promise.reject(error);
    }
  );

  return client;
}

/**
 * Wraps an Axios call and maps network errors to ElenxisError.
 */
export async function safeRequest<T>(
  fn: () => Promise<T>,
  errorCode: 'SUBMIT_FAILED' | 'POLL_FAILED' | 'PROOF_FETCH_FAILED' | 'NETWORK_ERROR',
  context: string
): Promise<T> {
  try {
    return await fn();
  } catch (err) {
    const message =
      err instanceof AxiosError
        ? `${context}: ${err.response?.data?.error ?? err.message}`
        : `${context}: ${String(err)}`;
    throw new VeilError(errorCode, message, err);
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}