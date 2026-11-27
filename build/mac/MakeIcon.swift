// Draws the app icon from Pip's own pixels:  MakeIcon <pet.json> <out.iconset>
// A warm rounded tile (the macOS icon grid: 824 of 1024 points, corner 185) with Pip standing
// in the middle, crisp at every size because each size is drawn, not scaled.
import AppKit

let args = CommandLine.arguments
let pet = try! JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: args[1]))) as! [String: Any]
let out = URL(fileURLWithPath: args[2])
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

let palette = pet["palette"] as! [String: String]
let anims = pet["animations"] as! [String: Any]
let idle = anims["idle"] as! [String: Any]
let frame = (idle["frames"] as! [[String]])[0]

func color(_ s: String) -> CGColor? {
    if s == "transparent" { return nil }
    if s.hasPrefix("#") {
        let hex = String(s.dropFirst())
        var v: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&v)
        if hex.count == 6 { return CGColor(srgbRed: CGFloat((v >> 16) & 255) / 255, green: CGFloat((v >> 8) & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: 1) }
        if hex.count == 8 { return CGColor(srgbRed: CGFloat((v >> 24) & 255) / 255, green: CGFloat((v >> 16) & 255) / 255, blue: CGFloat((v >> 8) & 255) / 255, alpha: CGFloat(v & 255) / 255) }
    }
    if s.hasPrefix("rgba(") {
        let n = s.dropFirst(5).dropLast().split(separator: ",").map { Double($0.trimmingCharacters(in: .whitespaces)) ?? 0 }
        return CGColor(srgbRed: n[0] / 255, green: n[1] / 255, blue: n[2] / 255, alpha: n.count > 3 ? n[3] : 1)
    }
    return nil
}

// The pixels that are actually Pip (skip the soft ground shadow; the icon draws its own).
var minX = Int.max, maxX = -1, minY = Int.max, maxY = -1
for (y, row) in frame.enumerated() {
    for (x, ch) in row.enumerated() where ch != "." && ch != " " && ch != "s" && color(palette[String(ch)] ?? "transparent") != nil {
        minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
    }
}
let bodyW = maxX - minX + 1, bodyH = maxY - minY + 1

func draw(_ px: Int) -> NSBitmapImageRep {
    let S = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
                               hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let c = NSGraphicsContext.current!.cgContext
    let k = S / 1024
    // Tile
    let tile = CGRect(x: 100 * k, y: 100 * k, width: 824 * k, height: 824 * k)
    let path = CGPath(roundedRect: tile, cornerWidth: 185 * k, cornerHeight: 185 * k, transform: nil)
    c.saveGState()
    c.setShadow(offset: CGSize(width: 0, height: -10 * k), blur: 24 * k, color: CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.28))
    c.addPath(path)
    c.setFillColor(CGColor(srgbRed: 0.99, green: 0.93, blue: 0.87, alpha: 1))
    c.fillPath()
    c.restoreGState()
    c.saveGState()
    c.addPath(path)
    c.clip()
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let g = CGGradient(colorsSpace: space, colors: [CGColor(srgbRed: 1.0, green: 0.96, blue: 0.91, alpha: 1), CGColor(srgbRed: 0.97, green: 0.82, blue: 0.70, alpha: 1)] as CFArray, locations: [0, 1])!
    c.drawLinearGradient(g, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])
    // Ground glow + shadow under Pip
    let glow = CGGradient(colorsSpace: space, colors: [CGColor(srgbRed: 0.85, green: 0.47, blue: 0.34, alpha: 0.28), CGColor(srgbRed: 0.85, green: 0.47, blue: 0.34, alpha: 0)] as CFArray, locations: [0, 1])!
    c.drawRadialGradient(glow, startCenter: CGPoint(x: S / 2, y: 300 * k), startRadius: 0, endCenter: CGPoint(x: S / 2, y: 300 * k), endRadius: 380 * k, options: [])
    c.restoreGState()

    // Pip: whole-number pixel size so it stays sharp.
    let cell = max(1, floor(min(560 * k / CGFloat(bodyW), 470 * k / CGFloat(bodyH))))
    let w = CGFloat(bodyW) * cell, h = CGFloat(bodyH) * cell
    let ox = floor((S - w) / 2), oy = floor(300 * k)   // feet sit a little below the middle
    c.setFillColor(CGColor(srgbRed: 0.45, green: 0.2, blue: 0.1, alpha: 0.22))
    c.fillEllipse(in: CGRect(x: ox + w * 0.08, y: oy - cell * 0.9, width: w * 0.84, height: cell * 1.6))
    c.interpolationQuality = .none
    for (y, row) in frame.enumerated() where y >= minY && y <= maxY {
        for (x, ch) in row.enumerated() where x >= minX && x <= maxX && ch != "s" {
            guard let col = color(palette[String(ch)] ?? "transparent") else { continue }
            c.setFillColor(col)
            c.fill(CGRect(x: ox + CGFloat(x - minX) * cell, y: oy + CGFloat(maxY - y) * cell, width: cell, height: cell))
        }
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

for (size, scale) in [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)] {
    let name = scale == 1 ? "icon_\(size)x\(size).png" : "icon_\(size)x\(size)@2x.png"
    try! draw(size * scale).representation(using: .png, properties: [:])!.write(to: out.appendingPathComponent(name))
}
