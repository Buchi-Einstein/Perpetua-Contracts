import { createContext, useContext, useMemo, type ReactNode } from 'react';
import {
  createPerpetuaClient,
  type PerpetuaClient,
  type PerpetuaClientOptions,
} from './rpc.js';

const PerpetuaContext = createContext<PerpetuaClient | null>(null);

/** Mount once near the app root to provide an RPC client to every stream hook. */
export function PerpetuaProvider({
  children,
  ...opts
}: PerpetuaClientOptions & { children?: ReactNode }) {
  const client = useMemo(
    () => createPerpetuaClient(opts),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [opts.rpcUrl, opts.contractId],
  );
  return <PerpetuaContext.Provider value={client}>{children}</PerpetuaContext.Provider>;
}

/** Access the Perpetua client configured by [`PerpetuaProvider`]. */
export function usePerpetua(): PerpetuaClient {
  const client = useContext(PerpetuaContext);
  if (!client) {
    throw new Error('usePerpetua requires a <PerpetuaProvider> ancestor');
  }
  return client;
}