import { motion } from 'framer-motion'
import {
  ArrowRight,
  Bot,
  Braces,
  Download,
  Github,
  Lock,
  ShieldCheck,
  Sparkles,
  Star,
  Table2,
  Terminal,
} from 'lucide-react'

const REPO = 'https://github.com/ntxinh/Datara'
const RELEASES = `${REPO}/releases`
const spring = { type: 'spring', stiffness: 100, damping: 20 }

const headline = ['SQL', 'Server,', 'finally', 'native', 'on', 'Linux.']

/* ---------- ambient mesh gradients ---------- */
function Ambient() {
  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 -z-10 overflow-hidden">
      <motion.div
        className="absolute -top-40 -left-40 h-[34rem] w-[34rem] rounded-full bg-neon-cyan/20 blur-[140px]"
        animate={{ x: [0, 60, 0], y: [0, 40, 0] }}
        transition={{ duration: 24, repeat: Infinity, ease: 'easeInOut' }}
      />
      <motion.div
        className="absolute top-1/3 -right-48 h-[38rem] w-[38rem] rounded-full bg-neon-purple/20 blur-[150px]"
        animate={{ x: [0, -70, 0], y: [0, 60, 0] }}
        transition={{ duration: 28, repeat: Infinity, ease: 'easeInOut' }}
      />
      <motion.div
        className="absolute -bottom-56 left-1/4 h-[36rem] w-[36rem] rounded-full bg-neon-orange/15 blur-[150px]"
        animate={{ x: [0, 50, 0], y: [0, -50, 0] }}
        transition={{ duration: 32, repeat: Infinity, ease: 'easeInOut' }}
      />
    </div>
  )
}

/* ---------- navbar ---------- */
function Navbar() {
  return (
    <header className="fixed inset-x-0 top-0 z-50">
      <div className="mx-auto mt-4 flex max-w-6xl items-center justify-between rounded-2xl border border-white/10 bg-white/5 px-5 py-3 backdrop-blur-xl sm:px-6">
        <a href="#" className="text-lg font-extrabold tracking-tight text-white">
          datara
        </a>
        <span className="hidden rounded-full border border-neon-cyan/30 bg-neon-cyan/10 px-3 py-1 font-mono text-[11px] tracking-widest text-neon-cyan sm:block">
          v0.1.0 · MVP
        </span>
        <a
          href={RELEASES}
          className="group inline-flex items-center gap-2 rounded-2xl border border-white/20 bg-white/10 px-4 py-2 text-sm font-semibold text-white transition hover:border-neon-cyan/50 hover:shadow-[0_0_24px_rgba(0,240,255,0.25)]"
        >
          Get Datara
          <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" />
        </a>
      </div>
    </header>
  )
}

/* ---------- hero ---------- */
function MockupCard() {
  const rows = [
    ['1', 'nvarchar', 'id', 'PRIMARY KEY'],
    ['2', 'int', 'row_count', 'NOT NULL'],
    ['3', 'datetime2', 'created_at', ''],
    ['4', 'bit', 'is_active', 'DEFAULT 1'],
  ]
  return (
    <motion.div
      animate={{ y: [0, -15, 0] }}
      transition={{ duration: 6, repeat: Infinity, ease: 'easeInOut' }}
      className="w-full max-w-md rounded-2xl border border-white/10 bg-white/5 p-4 shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] backdrop-blur-xl"
      style={{ transform: 'perspective(1200px) rotateX(6deg) rotateY(-8deg)' }}
    >
      {/* window bar */}
      <div className="mb-4 flex items-center gap-2 border-b border-white/10 pb-3">
        <span className="size-2.5 rounded-full bg-neon-orange/70" />
        <span className="size-2.5 rounded-full bg-neon-cyan/70" />
        <span className="size-2.5 rounded-full bg-neon-purple/70" />
        <span className="ml-3 font-mono text-[10px] text-white/40">datara — mssql://prod-main</span>
      </div>
      <div className="flex gap-3">
        {/* sidebar */}
        <div className="hidden w-24 shrink-0 flex-col gap-2 border-r border-white/10 pr-3 sm:flex">
          {['dbo.users', 'dbo.orders', 'dbo.logs'].map((t) => (
            <span key={t} className="rounded-md bg-white/5 px-2 py-1.5 font-mono text-[9px] text-white/50">
              {t}
            </span>
          ))}
          <div className="mt-2 h-1.5 w-3/4 rounded bg-white/10" />
          <div className="h-1.5 w-1/2 rounded bg-white/10" />
        </div>
        {/* main pane */}
        <div className="min-w-0 flex-1">
          <div className="mb-3 flex items-center gap-2 rounded-lg border border-neon-cyan/30 bg-neon-cyan/5 px-3 py-2">
            <Sparkles className="size-3.5 shrink-0 text-neon-cyan" />
            <span className="truncate font-mono text-[10px] text-neon-cyan">
              SELECT TOP 1000 * FROM dbo.users
            </span>
          </div>
          <table className="w-full font-mono text-[9px] text-white/60">
            <tbody>
              {rows.map((r) => (
                <tr key={r[0]} className="border-b border-white/5">
                  {r.map((c, i) => (
                    <td key={i} className={`py-1.5 pr-2 ${i === 0 ? 'text-white/25' : ''}`}>
                      {c}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-3 flex items-center justify-between font-mono text-[9px] text-white/30">
            <span>4 rows · 12 ms</span>
            <span className="flex items-center gap-1 text-neon-cyan/70">
              <Lock className="size-2.5" /> secret service
            </span>
          </div>
        </div>
      </div>
    </motion.div>
  )
}

function Hero() {
  return (
    <section className="relative mx-auto flex min-h-screen max-w-6xl flex-col items-center justify-center px-6 pt-32 pb-24 text-center">
      <motion.p
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={spring}
        className="mb-6 font-mono text-xs tracking-[0.3em] text-white/40 uppercase"
      >
        Rust · Slint · Wayland
      </motion.p>
      <h1 className="max-w-4xl text-5xl font-black tracking-tight sm:text-7xl">
        {headline.map((word, i) => (
          <motion.span
            key={i}
            initial={{ opacity: 0, y: 40, filter: 'blur(8px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            transition={{ ...spring, delay: 0.08 * i }}
            className={`mr-3 inline-block ${
              i >= 3
                ? 'bg-gradient-to-r from-neon-cyan via-neon-purple to-neon-orange bg-clip-text text-transparent'
                : 'text-white'
            }`}
          >
            {word}
          </motion.span>
        ))}
      </h1>
      <motion.p
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...spring, delay: 0.5 }}
        className="mt-6 max-w-xl text-lg text-white/50"
      >
        A fast, native MSSQL client for Linux. One binary, no Electron, no
        accounts — credentials live in your Secret Service.
      </motion.p>
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...spring, delay: 0.65 }}
        className="mt-10 flex flex-wrap items-center justify-center gap-4"
      >
        <motion.a
          href={RELEASES}
          whileHover={{ scale: 1.05 }}
          whileTap={{ scale: 0.95 }}
          transition={spring}
          className="inline-flex items-center gap-2 rounded-2xl border border-neon-cyan/40 bg-gradient-to-r from-neon-cyan/20 to-neon-purple/20 px-7 py-3.5 font-semibold text-white shadow-[0_0_40px_rgba(0,240,255,0.3),inset_0_1px_0_rgba(255,255,255,0.15)]"
        >
          <Download className="size-4" />
          Download for Linux
        </motion.a>
        <motion.a
          href={REPO}
          whileHover={{ scale: 1.05 }}
          whileTap={{ scale: 0.95 }}
          transition={spring}
          className="inline-flex items-center gap-2 rounded-2xl border border-white/15 bg-white/5 px-7 py-3.5 font-semibold text-white/80 backdrop-blur-xl transition hover:border-white/30"
        >
          <Github className="size-4" />
          Source
        </motion.a>
      </motion.div>
      <motion.div
        initial={{ opacity: 0, y: 60 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ...spring, delay: 0.85 }}
        className="mt-16"
      >
        <MockupCard />
      </motion.div>
    </section>
  )
}

/* ---------- marquee ---------- */
const MARQUEE_ITEMS = [
  'MSSQL',
  'TDS PROTOCOL',
  'RUST',
  'SLINT UI',
  'WAYLAND',
  'SECRET SERVICE',
  'TOKIO',
  'MCP SERVER',
  'SINGLE BINARY',
  'GPL-3.0',
]

function Marquee() {
  const items = [...MARQUEE_ITEMS, ...MARQUEE_ITEMS]
  return (
    <section aria-label="Technology stack" className="relative border-y border-white/5 py-6 opacity-40">
      <div className="flex overflow-hidden [mask-image:linear-gradient(to_right,transparent,black_15%,black_85%,transparent)]">
        <div className="animate-marquee flex shrink-0 items-center gap-10 pr-10">
          {items.map((item, i) => (
            <span key={i} className="flex items-center gap-10 font-mono text-sm tracking-widest whitespace-nowrap text-white/70">
              {item}
              <span className="size-1 rotate-45 bg-white/40" />
            </span>
          ))}
        </div>
      </div>
    </section>
  )
}

/* ---------- bento ---------- */
const FEATURES = [
  {
    icon: Table2,
    title: 'Bounded table preview',
    body: 'Browsing is SELECT TOP N — 1000 rows by default, never an unbounded SELECT *. Edit values directly in the data grid.',
    span: 'md:col-span-2',
  },
  {
    icon: Braces,
    title: 'A real SQL editor',
    body: 'T-SQL syntax highlighting and schema-aware completion, backed by your live connection.',
    span: '',
  },
  {
    icon: ShieldCheck,
    title: 'Secrets stay secret',
    body: 'Passwords live only in Secret Service — never in SQLite, logs, or error strings. TLS without silent downgrade.',
    span: '',
  },
  {
    icon: Bot,
    title: 'Speaks MCP',
    body: 'A bundled MCP server reuses the app services, so AI agents query through the same safe path you do.',
    span: 'md:col-span-2',
  },
]

function FeatureCard({ icon: Icon, title, body, span, index }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 50 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: '-80px' }}
      transition={{ ...spring, delay: index * 0.08 }}
      whileHover={{ rotate: -0.6, scale: 1.015 }}
      className={`group relative overflow-hidden rounded-2xl border border-white/10 bg-white/5 p-7 backdrop-blur-xl ${span}`}
    >
      <div className="absolute -top-20 -right-20 size-48 rounded-full bg-neon-cyan/0 blur-3xl transition duration-500 group-hover:bg-neon-cyan/15" />
      <Icon className="mb-5 size-6 text-neon-cyan" />
      <h3 className="mb-2 text-lg font-bold text-white">{title}</h3>
      <p className="text-sm leading-relaxed text-white/50">{body}</p>
    </motion.div>
  )
}

function Bento() {
  return (
    <section className="mx-auto max-w-6xl px-6 py-28">
      <motion.div
        initial={{ opacity: 0, y: 30 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true }}
        transition={spring}
        className="mb-12"
      >
        <p className="mb-3 font-mono text-xs tracking-[0.3em] text-neon-purple uppercase">
          Built different
        </p>
        <h2 className="max-w-xl text-3xl font-extrabold tracking-tight text-white sm:text-4xl">
          Native means native. No compromises.
        </h2>
      </motion.div>
      <div className="grid gap-4 md:grid-cols-3">
        {FEATURES.map((f, i) => (
          <FeatureCard key={f.title} {...f} index={i} />
        ))}
      </div>
    </section>
  )
}

/* ---------- free / conversion ---------- */
function FreePanel() {
  return (
    <section className="mx-auto max-w-6xl px-6 pb-28">
      <motion.div
        initial={{ opacity: 0, y: 50 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true, margin: '-80px' }}
        transition={spring}
        className="relative overflow-hidden rounded-3xl p-px"
        style={{
          background:
            'linear-gradient(135deg, rgba(0,240,255,0.6), rgba(176,38,255,0.6), rgba(255,77,77,0.6))',
        }}
      >
        <div className="absolute -top-32 left-1/2 h-64 w-2/3 -translate-x-1/2 rounded-full bg-neon-purple/25 blur-[100px]" />
        <div className="relative rounded-3xl bg-obsidian/90 px-8 py-16 text-center backdrop-blur-xl">
          <p className="mb-4 font-mono text-xs tracking-[0.3em] text-white/40 uppercase">
            Pricing
          </p>
          <p className="bg-gradient-to-r from-neon-cyan via-white to-neon-purple bg-clip-text font-mono text-7xl font-bold text-transparent sm:text-8xl">
            $0
          </p>
          <p className="mt-6 font-mono text-sm text-white/60">
            GPL-3.0 · no accounts · no telemetry · forever
          </p>
          <motion.a
            href={REPO}
            whileHover={{ scale: 1.05 }}
            whileTap={{ scale: 0.95 }}
            transition={spring}
            className="mt-10 inline-flex items-center gap-2 rounded-2xl border border-neon-purple/40 bg-gradient-to-r from-neon-purple/20 to-neon-cyan/20 px-7 py-3.5 font-semibold text-white shadow-[0_0_40px_rgba(176,38,255,0.3),inset_0_1px_0_rgba(255,255,255,0.15)]"
          >
            <Star className="size-4" />
            Star on GitHub
          </motion.a>
        </div>
      </motion.div>
    </section>
  )
}

/* ---------- footer ---------- */
function Footer() {
  const links = [
    ['ntxinh/Datara', REPO],
    ['releases', RELEASES],
    ['issues', `${REPO}/issues`],
    ['docs', `${REPO}/tree/main/docs`],
  ]
  return (
    <footer className="border-t border-white/10">
      <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 py-10 font-mono text-xs text-white/40 sm:flex-row">
        <span>© 2026 ntxinh — GPL-3.0</span>
        <nav className="flex flex-wrap items-center gap-6">
          {links.map(([label, href]) => (
            <a key={label} href={href} className="transition hover:text-neon-cyan">
              {label}
            </a>
          ))}
        </nav>
        <span className="flex items-center gap-1.5">
          <Terminal className="size-3.5" />
          built with rust
        </span>
      </div>
    </footer>
  )
}

export default function App() {
  return (
    <div className="min-h-screen bg-obsidian">
      <Ambient />
      <Navbar />
      <main>
        <Hero />
        <Marquee />
        <Bento />
        <FreePanel />
      </main>
      <Footer />
    </div>
  )
}
