import { ElenxisErrorCode } from '../src/types';
/**
 * All errors thrown by the Elenxis SDK are instances of ElenxisError.
 * Check the `code` field to handle specific failure modes.
 *
 * @example
 * try {
 *   await client.verifyInference(...)
 * } catch (err) {
 *   if (err instanceof ElenxisError && err.code === 'TIMEOUT') {
 *     // handle timeout
 *   }
 * }
 */
export declare class ElenxisError extends Error {
    readonly code: ElenxisErrorCode;
    readonly cause?: unknown;
    constructor(code: ElenxisErrorCode, message: string, cause?: unknown);
}
//# sourceMappingURL=errors.d.ts.map