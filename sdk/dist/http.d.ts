import { AxiosInstance } from 'axios';
/**
 * Creates a pre-configured Axios instance with exponential backoff retry.
 */
export declare function createHttpClient(baseURL: string, maxRetries?: number): AxiosInstance;
/**
 * Wraps an Axios call and maps network errors to ElenxisError.
 */
export declare function safeRequest<T>(fn: () => Promise<T>, errorCode: 'SUBMIT_FAILED' | 'POLL_FAILED' | 'PROOF_FETCH_FAILED' | 'NETWORK_ERROR', context: string): Promise<T>;
//# sourceMappingURL=http.d.ts.map