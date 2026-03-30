"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ElenxisError = void 0;
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
class ElenxisError extends Error {
    constructor(code, message, cause) {
        super(message);
        this.name = 'ElenxisError';
        this.code = code;
        this.cause = cause;
        // Restore prototype chain (required when extending built-in classes in TS)
        Object.setPrototypeOf(this, new.target.prototype);
    }
}
exports.ElenxisError = ElenxisError;
//# sourceMappingURL=errors.js.map