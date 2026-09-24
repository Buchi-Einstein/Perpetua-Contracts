export interface BatchStreamRow {
  recipient: string;
  amount: string;
  durationDays: string;
  cliffDays: string;
}

export interface ParsedBatchStream {
  recipient: string;
  amount: i128;
  durationDays: number;
  cliffDays: number;
}

export function parseCsv(input: string): ParsedBatchStream[] {
  const lines = input.trim().split(/\r?\n/);
  if (lines.length < 2) {
    throw new Error("CSV must contain a header row and at least one data row");
  }

  const header = lines[0].split(",").map((cell) => cell.trim().toLowerCase());
  const required = ["recipient", "amount", "duration_days", "cliff_days"];
  for (const field of required) {
    if (!header.includes(field)) {
      throw new Error(`Missing required CSV column: ${field}`);
    }
  }

  const results: ParsedBatchStream[] = [];
  for (let i = 1; i < lines.length; i++) {
    const cells = lines[i].split(",").map((cell) => cell.trim());
    if (cells.length !== header.length) {
      throw new Error(`Row ${i + 1} has ${cells.length} columns, expected ${header.length}`);
    }

    const row: Record<string, string> = {};
    header.forEach((name, idx) => {
      row[name] = cells[idx];
    });

    const amount = BigInt(row.amount);
    const durationDays = Number(row.duration_days);
    const cliffDays = Number(row.cliff_days);

    if (amount <= 0n) {
      throw new Error(`Row ${i + 1}: amount must be positive`);
    }
    if (durationDays <= 0) {
      throw new Error(`Row ${i + 1}: duration_days must be positive`);
    }
    if (cliffDays < 0 || cliffDays > durationDays) {
      throw new Error(`Row ${i + 1}: cliff_days must be between 0 and duration_days`);
    }

    results.push({
      recipient: row.recipient,
      amount: amount as i128,
      durationDays,
      cliffDays,
    });
  }

  return results;
}

export interface BuildBatchCreateOptions {
  sender: string;
  token: string;
  startTime: number;
  cancellable: boolean;
  pausable: boolean;
  transferable: boolean;
}

export interface BatchCreateTransaction {
  source: string;
  operations: Array<{
    createStream: {
      sender: string;
      recipient: string;
      token: string;
      deposit: bigint;
      startTime: number;
      endTime: number;
      cliffTime: number;
      cancellable: boolean;
      pausable: boolean;
      transferable: boolean;
    };
  }>;
}

export function buildBatchCreateTransaction(
  rows: ParsedBatchStream[],
  options: BuildBatchCreateOptions,
): BatchCreateTransaction {
  if (rows.length === 0) {
    throw new Error("No rows to build into a transaction");
  }
  if (rows.length > 16) {
    throw new Error("Batch size exceeds MAX_BATCH_SIZE of 16; split client-side");
  }

  const operations = rows.map((row) => ({
    createStream: {
      sender: options.sender,
      recipient: row.recipient,
      token: options.token,
      deposit: row.amount,
      startTime: options.startTime,
      endTime: options.startTime + row.durationDays * 86_400,
      cliffTime: options.startTime + row.cliffDays * 86_400,
      cancellable: options.cancellable,
      pausable: options.pausable,
      transferable: options.transferable,
    },
  }));

  return {
    source: options.sender,
    operations,
  };
}
