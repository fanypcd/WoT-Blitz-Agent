// 本机资产面静态服务：把 `release/asset_pack`（export_asset_pack.py 的产物）按消费方
// assetProvider 期望的布局伺服，并带上 CORS 头——WotbTools dev server（5173）跨源取
// GLB / 地图 / 坦克数据时浏览器才放行。
//
// 为什么需要：WotbTools 前端是 client-only + remote 资产面设计，dev 下没有构建期
// `VITE_ASSET_BASE_URL` 时 `assetProvider` 会显式报「资产源未配置」，地图地形 /
// 车模 GLB / 坦克 JSON 一律不加载（回放解析本身不依赖资产，所以现象是"只有模型没了"）。
// 标准本机联调姿势（两侧仓库都记在文档里）：
//   1) node scripts/serve_asset_pack.mjs 8123            # 本仓，伺服 release/asset_pack
//   2) frontend/.env.local: VITE_ASSET_BASE_URL=http://127.0.0.1:8123   # WotbTools 仓
//   3) npm run dev → http://localhost:5173/?view=agent-replay&agentViews=1
//
// 用法：node scripts/serve_asset_pack.mjs [port] [packDir]
import { createReadStream, existsSync, statSync } from 'node:fs'
import { createServer } from 'node:http'
import { extname, join, normalize, resolve } from 'node:path'

const port = Number(process.argv[2] || 8123)
const root = resolve(process.argv[3] || 'release/asset_pack')

const TYPES = {
  '.json': 'application/json', '.glb': 'model/gltf-binary', '.webp': 'image/webp',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.bin': 'application/octet-stream',
  '.u16.bin': 'application/octet-stream', '.wotbreplay': 'application/octet-stream',
}

createServer((req, res) => {
  const urlPath = decodeURIComponent((req.url || '/').split('?')[0])
  const file = join(root, normalize(urlPath).replace(/^([/\\])+/, ''))
  if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, { 'Access-Control-Allow-Origin': '*' })
    res.end('not found: ' + urlPath)
    return
  }
  const size = statSync(file).size
  const ext = extname(file).toLowerCase()
  res.writeHead(200, {
    'Content-Type': TYPES[ext] || 'application/octet-stream',
    'Content-Length': size,
    'Access-Control-Allow-Origin': '*',
    'Accept-Ranges': 'bytes',
    'Cache-Control': 'no-cache',
  })
  if (req.method === 'HEAD') { res.end(); return }
  createReadStream(file).pipe(res)
}).listen(port, '127.0.0.1', () => {
  console.log(`asset pack: http://127.0.0.1:${port}/  (root=${root})`)
})
