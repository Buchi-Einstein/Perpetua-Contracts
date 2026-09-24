type Stream = {
  id: number;
  recipient: string;
  deposited: number;
  vested: number;
  ratePerSec: number;
  status: "active" | "paused" | "cancelled";
};

// Sample ledger — the reference UI wires this to soroban simulation once the
// contract bindings land (stage 5/6); here it is static proof of the layout.
const streams: Stream[] = [
  { id: 1, recipient: "GALAX…7QKD", deposited: 48_000, vested: 21_830, ratePerSec: 0.251, status: "active" },
  { id: 2, recipient: "GBLOOM…2HZX", deposited: 12_500, vested: 12_500, ratePerSec: 0.062, status: "active" },
  { id: 3, recipient: "GNOVA…9PQA", deposited: 6_000, vested: 1_120, ratePerSec: 0.033, status: "paused" },
];

function fmt(n: number) {
  return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
}

export default function Dashboard() {
  const totalVested = streams.reduce((sum, s) => sum + s.vested, 0);
  const totalDeposited = streams.reduce((sum, s) => sum + s.deposited, 0);

  return (
    <main className="mx-auto max-w-5xl px-6 py-12">
      <header className="flex items-center justify-between">
        <div>
          <p className="text-xs uppercase tracking-widest text-sky-400">Perpetua</p>
          <h1 className="mt-1 text-3xl font-semibold">Streaming Payroll</h1>
        </div>
        <span className="cursor-not-allowed rounded-lg bg-slate-800 px-4 py-2 text-sm font-medium text-slate-500">
          New stream (stage 6)
        </span>
      </header>

      <section className="mt-10 grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5">
          <p className="text-sm text-slate-400">Active streams</p>
          <p className="mt-1 text-3xl font-semibold">{streams.filter((s) => s.status === "active").length}</p>
        </div>
        <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5">
          <p className="text-sm text-slate-400">Vested</p>
          <p className="mt-1 text-3xl font-semibold">${fmt(totalVested)}</p>
        </div>
        <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-5">
          <p className="text-sm text-slate-400">Deposited</p>
          <p className="mt-1 text-3xl font-semibold">${fmt(totalDeposited)}</p>
        </div>
      </section>

      <section className="mt-10 overflow-hidden rounded-xl border border-slate-800">
        <div className="grid grid-cols-12 gap-4 border-b border-slate-800 bg-slate-900/80 px-5 py-3 text-xs uppercase tracking-wider text-slate-500">
          <span className="col-span-3">Stream</span>
          <span className="col-span-2">Recipient</span>
          <span className="col-span-2">Rate /s</span>
          <span className="col-span-3">Vested</span>
          <span className="col-span-2">Status</span>
        </div>
        {streams.map((s) => {
          const pct = Math.min(100, Math.round((s.vested / s.deposited) * 100));
          return (
            <div
              key={s.id}
              className="grid grid-cols-12 items-center gap-4 border-b border-slate-800/60 px-5 py-4 last:border-0"
            >
              <span className="col-span-3 font-medium">
                #{s.id} · ${fmt(s.deposited)}
              </span>
              <span className="col-span-2 text-slate-400">{s.recipient}</span>
              <span className="col-span-2 text-slate-400">${s.ratePerSec}</span>
              <div className="col-span-3">
                <div className="h-2 rounded-full bg-slate-800">
                  <div className="h-2 rounded-full bg-sky-400" style={{ width: `${pct}%` }} />
                </div>
                <p className="mt-1 text-xs text-slate-500">
                  ${fmt(s.vested)} · {pct}%
                </p>
              </div>
              <span
                className={`col-span-2 w-fit rounded-full px-2 py-0.5 text-xs capitalize ${
                  s.status === "active"
                    ? "bg-emerald-500/10 text-emerald-300"
                    : s.status === "paused"
                      ? "bg-amber-500/10 text-amber-300"
                      : "bg-rose-500/10 text-rose-300"
                }`}
              >
                {s.status}
              </span>
            </div>
          );
        })}
      </section>

      <p className="mt-8 text-center text-xs text-slate-600">
        Reference UI for Perpetua — integrates with the v1 contract ABI via the TypeScript SDK
        (docs/ABI.md). Not a product.
      </p>
    </main>
  );
}