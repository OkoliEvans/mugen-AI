"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.createHttpClient = createHttpClient;
exports.safeRequest = safeRequest;
const axios_1 = __importStar(require("axios"));
const errors_1 = require("./errors");
const DEFAULT_MAX_RETRIES = 3;
const RETRY_DELAY_MS = 500;
/**
 * Creates a pre-configured Axios instance with exponential backoff retry.
 */
function createHttpClient(baseURL, maxRetries = DEFAULT_MAX_RETRIES) {
    const client = axios_1.default.create({
        baseURL,
        headers: { 'Content-Type': 'application/json' },
        timeout: 30000,
    });
    client.interceptors.response.use((response) => response, async (error) => {
        const config = error.config;
        if (!config)
            return Promise.reject(error);
        config._retryCount = config._retryCount ?? 0;
        // Only retry on network errors or 5xx responses
        const isRetryable = !error.response || (error.response.status >= 500 && error.response.status < 600);
        if (isRetryable && config._retryCount < maxRetries) {
            config._retryCount += 1;
            const delay = RETRY_DELAY_MS * Math.pow(2, config._retryCount - 1);
            await sleep(delay);
            return client(config);
        }
        return Promise.reject(error);
    });
    return client;
}
/**
 * Wraps an Axios call and maps network errors to ElenxisError.
 */
async function safeRequest(fn, errorCode, context) {
    try {
        return await fn();
    }
    catch (err) {
        const message = err instanceof axios_1.AxiosError
            ? `${context}: ${err.response?.data?.error ?? err.message}`
            : `${context}: ${String(err)}`;
        throw new errors_1.ElenxisError(errorCode, message, err);
    }
}
function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
//# sourceMappingURL=http.js.map