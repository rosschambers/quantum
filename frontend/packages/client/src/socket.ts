// Socket transport for Node.js environments (optional for now — stub only).
// This will be implemented when needed for Node-based clients.

import type { Transport } from './transport';

export function createSocketTransport(_socketPath: string): Transport {
  throw new Error('Socket transport not yet implemented');
}
