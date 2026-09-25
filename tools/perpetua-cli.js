#!/usr/bin/env node

import { parseArgs } from "node:util";

import {
  buildCreateOperation,
  buildWithdrawOperation,
  buildInspectQuery,
  buildExtendOperation,
  buildBatchCreateFromCsv,
  type CreateCliInput,
  type WithdrawCliInput,
  type InspectCliInput,
  type ExtendCliInput,
} from "./perpetua-cli.ts";

type Command = "create" | "withdraw" | "inspect" | "extend" | "batch-create";

interface Args {
  _: string[];
  command?: Command;
  recipient?: string;
  amount?: string;
  "duration-days"?: string;
  "cliff-days"?: string;
  "start-time"?: string;
  token?: string;
  "stream-id"?: string;
  csv?: string;
  sender?: string;
  help?: boolean;
}

function printHelp() {
  console.log(`Perpetua CLI

Usage:
  perpetua create --recipient <address> --amount <stroops> --duration-days <days> [--cliff-days <days>] [--start-time <unix>] [--token <address>]
  perpetua withdraw --stream-id <id> [--amount <stroops>]
  perpetua inspect --stream-id <id>
  perpetua extend --stream-id <id> [--ttl-ledgers <ledgers>]
  perpetua batch-create --csv <path> --sender <address> --token <address> [--start-time <unix>]
`);
}

function toNumber(value: string | undefined, label: string): number {
  if (value === undefined) {
    throw new Error(`Missing required option: --${label}`);
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`--${label} must be a non-negative number`);
  }
  return parsed;
}

function toBigInt(value: string | undefined, label: string): bigint {
  if (value === undefined) {
    throw new Error(`Missing required option: --${label}`);
  }
  const parsed = BigInt(value);
  if (parsed <= 0n) {
    throw new Error(`--${label} must be positive`);
  }
  return parsed;
}

async function main() {
  const { values, positionals } = parseArgs<Args>({
    allowPositionals: true,
    options: {
      command: { type: "string" },
      recipient: { type: "string" },
      amount: { type: "string" },
      "duration-days": { type: "string" },
      "cliff-days": { type: "string" },
      "start-time": { type: "string" },
      token: { type: "string" },
      "stream-id": { type: "string" },
      csv: { type: "string" },
      sender: { type: "string" },
      help: { type: "boolean", default: false },
    },
  });

  if (values.help || !values.command) {
    printHelp();
    process.exit(values.help ? 0 : 1);
  }

  const command = values.command as Command;

  try {
    switch (command) {
      case "create": {
        const input: CreateCliInput = {
          recipient: values.recipient ?? positionals[1],
          amount: values.amount ?? positionals[2],
          durationDays: toNumber(values["duration-days"], "duration-days"),
          cliffDays: values["cliff-days"] ? toNumber(values["cliff-days"], "cliff-days") : undefined,
          startTime: values["start-time"] ? toNumber(values["start-time"], "start-time") : undefined,
          token: values.token,
        };
        console.log(JSON.stringify(buildCreateOperation(input), null, 2));
        break;
      }
      case "withdraw": {
        const input: WithdrawCliInput = {
          streamId: toNumber(values["stream-id"], "stream-id"),
          amount: values.amount,
        };
        console.log(JSON.stringify(buildWithdrawOperation(input), null, 2));
        break;
      }
      case "inspect": {
        const input: InspectCliInput = {
          streamId: toNumber(values["stream-id"], "stream-id"),
        };
        console.log(JSON.stringify(buildInspectQuery(input), null, 2));
        break;
      }
      case "extend": {
        const input: ExtendCliInput = {
          streamId: toNumber(values["stream-id"], "stream-id"),
          ttlLedgers: values["ttl-ledgers"] ? toNumber(values["ttl-ledgers"], "ttl-ledgers") : undefined,
        };
        console.log(JSON.stringify(buildExtendOperation(input), null, 2));
        break;
      }
      case "batch-create": {
        const fs = await import("node:fs");
        const csvPath = values.csv ?? positionals[1];
        if (!csvPath) {
          throw new Error("--csv <path> is required for batch-create");
        }
        const csv = fs.readFileSync(csvPath, "utf8");
        const sender = values.sender;
        const token = values.token;
        if (!sender || !token) {
          throw new Error("--sender and --token are required for batch-create");
        }
        const startTime = values["start-time"] ? toNumber(values["start-time"], "start-time") : Math.floor(Date.now() / 1000);
        console.log(JSON.stringify(buildBatchCreateFromCsv(csv, sender, token, startTime), null, 2));
        break;
      }
      default:
        throw new Error(`Unknown command: ${command}`);
    }
  } catch (error) {
    console.error(`Error: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}

main();
