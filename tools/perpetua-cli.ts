import { parse, buildBatchCreateTransaction, type BatchCreateTransaction, type ParsedBatchStream } from "./sdk/batch_create.ts";

export type CliCommand = "create" | "withdraw" | "inspect" | "extend";

export interface CliContext {
  contractId: string;
  rpcUrl: string;
  networkPassphrase: string;
  sourceSecret?: string;
}

export interface CreateCliInput {
  recipient: string;
  amount: string;
  durationDays: number;
  cliffDays?: number;
  startTime?: number;
  token?: string;
  cancellable?: boolean;
  pausable?: boolean;
  transferable?: boolean;
}

export interface WithdrawCliInput {
  streamId: number;
  amount?: string;
}

export interface InspectCliInput {
  streamId: number;
}

export interface ExtendCliInput {
  streamId: number;
  ttlLedgers?: number;
}

export interface PerpetuaCliResult {
  ok: boolean;
  output: string;
  error?: string;
}

export function buildCreateOperation(input: CreateCliInput) {
  const startTime = input.startTime ?? Math.floor(Date.now() / 1000);
  const cliffTime = startTime + (input.cliffDays ?? 0) * 86_400;
  const endTime = startTime + input.durationDays * 86_400;

  return {
    createStream: {
      sender: "",
      recipient: input.recipient,
      token: input.token ?? "",
      deposit: BigInt(input.amount),
      startTime,
      endTime,
      cliffTime,
      cancellable: input.cancellable ?? true,
      pausable: input.pausable ?? true,
      transferable: input.transferable ?? true,
    },
  };
}

export function buildWithdrawOperation(input: WithdrawCliInput) {
  return {
    withdraw: {
      streamId: input.streamId,
      amount: input.amount ? BigInt(input.amount) : undefined,
    },
  };
}

export function buildInspectQuery(input: InspectCliInput) {
  return {
    inspect: {
      streamId: input.streamId,
    },
  };
}

export function buildExtendOperation(input: ExtendCliInput) {
  return {
    extend: {
      streamId: input.streamId,
      ttlLedgers: input.ttlLedgers,
    },
  };
}

export function buildBatchCreateFromCsv(
  csv: string,
  sender: string,
  token: string,
  startTime: number,
): BatchCreateTransaction {
  const rows = parse(csv);
  return buildBatchCreateTransaction(rows, {
    sender,
    token,
    startTime,
    cancellable: true,
    pausable: true,
    transferable: true,
  });
}

export function formatResult(result: PerpetuaCliResult): string {
  if (!result.ok) {
    return `Error: ${result.error}`;
  }
  return result.output;
}
