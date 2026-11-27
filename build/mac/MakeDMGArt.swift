// Makes what the disk image's window shows, into the folder the image is made from:
//   MakeDMGArt <stage folder>
// - .background/background.png and background@2x.png (build-mac.sh joins them into one tiff):
//   the app on the left, an arrow, Applications on the right, and under them, what to do if
//   macOS won't open it (it isn't notarized, so macOS says it can't check it).
// - "Open Security Settings": double-clicked, it opens Privacy & Security, where Open Anyway is.
// The window is 660 x 500 points; build-mac.sh puts the 112-point icons where this draws round
// them: the app at (170, 150), Applications at (490, 150), the shortcut at (545, 390).
import AppKit

let stage = URL(fileURLWithPath: CommandLine.arguments[1])
let W = 660.0, H = 500.0
let ink = NSColor(srgbRed: 0.32, green: 0.16, blue: 0.09, alpha: 1)

func draw(scale: Double) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W * scale), pixelsHigh: Int(H * scale), bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: W, height: H)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let c = NSGraphicsContext.current!.cgContext
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    func colour(_ r: Double, _ g: Double, _ b: Double, _ a: Double = 1) -> CGColor { CGColor(colorSpace: space, components: [r, g, b, a])! }
    func y(_ top: Double) -> Double { H - top } // points from the top left, as the Finder places icons
    func text(_ s: String, _ size: Double, _ weight: NSFont.Weight, _ alpha: Double, at rect: NSRect, centred: Bool = false) {
        let style = NSMutableParagraphStyle()
        style.alignment = centred ? .center : .left
        style.lineSpacing = 3
        NSAttributedString(string: s, attributes: [
            .font: NSFont.systemFont(ofSize: size, weight: weight),
            .foregroundColor: ink.withAlphaComponent(alpha),
            .paragraphStyle: style,
        ]).draw(in: rect)
    }

    // Warm cream, a little peach toward the edges (light, so the Finder's dark icon labels read).
    let bg = CGGradient(colorsSpace: space, colors: [colour(1, 0.98, 0.95), colour(0.98, 0.88, 0.8)] as CFArray, locations: [0, 1])!
    c.drawRadialGradient(bg, startCenter: CGPoint(x: W / 2, y: y(150)), startRadius: 0, endCenter: CGPoint(x: W / 2, y: y(150)), endRadius: 460, options: [.drawsAfterEndLocation])
    for x in [170.0, 490.0] {
        let glow = CGGradient(colorsSpace: space, colors: [colour(0.85, 0.47, 0.34, 0.18), colour(0.85, 0.47, 0.34, 0)] as CFArray, locations: [0, 1])!
        c.drawRadialGradient(glow, startCenter: CGPoint(x: x, y: y(150)), startRadius: 0, endCenter: CGPoint(x: x, y: y(150)), endRadius: 100, options: [])
    }

    // The arrow: a dotted pixel trail from the app, and a chunky pixel head pointing at Applications.
    let ay = y(150)
    for (i, x) in stride(from: 270.0, through: 352.0, by: 14.0).enumerated() {
        c.setFillColor(colour(0.85, 0.47, 0.34, 0.25 + Double(i) * 0.11))
        c.fill(CGRect(x: x, y: ay - 5, width: 10, height: 10))
    }
    c.setFillColor(colour(0.85, 0.47, 0.34, 0.95))
    for (dx, half) in [(0.0, 25.0), (10.0, 20.0), (20.0, 15.0), (30.0, 10.0), (40.0, 5.0)] {
        c.fill(CGRect(x: 364 + dx, y: ay - half, width: 10, height: half * 2))
    }
    text("Drag Promptagotchi into Applications", 15, .semibold, 0.85, at: NSRect(x: 0, y: y(282), width: W, height: 22), centred: true)

    // If it won't open: a panel with the three steps, the shortcut sitting in it on the right.
    let panel = CGRect(x: 24, y: y(486), width: W - 48, height: 172)
    c.addPath(CGPath(roundedRect: panel, cornerWidth: 16, cornerHeight: 16, transform: nil))
    c.setFillColor(colour(1, 1, 1, 0.72))
    c.fillPath()
    c.addPath(CGPath(roundedRect: panel.insetBy(dx: 0.5, dy: 0.5), cornerWidth: 16, cornerHeight: 16, transform: nil))
    c.setStrokeColor(colour(0.85, 0.47, 0.34, 0.25))
    c.setLineWidth(1)
    c.strokePath()
    text("macOS says it can't verify Promptagotchi?", 14, .semibold, 0.95, at: NSRect(x: 48, y: y(352), width: 420, height: 20))
    text("That's because it isn't from the App Store. To open it:", 12, .regular, 0.65, at: NSRect(x: 48, y: y(374), width: 420, height: 18))
    text("1.  Click Done on that message.\n2.  Double-click Open Security Settings →\n3.  Scroll down, click Open Anyway, then open\n     Promptagotchi from Applications again.",
         13, .medium, 0.9, at: NSRect(x: 48, y: y(472), width: 420, height: 90))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let background = stage.appendingPathComponent(".background")
try! FileManager.default.createDirectory(at: background, withIntermediateDirectories: true)
for (scale, name) in [(1.0, "background.png"), (2.0, "background@2x.png")] {
    try! draw(scale: scale).representation(using: .png, properties: [:])!.write(to: background.appendingPathComponent(name))
}

// The shortcut to Privacy & Security: a Finder alias of its settings pane, which opens System
// Settings right there, wearing System Settings' gear.
let shortcut = stage.appendingPathComponent("Open Security Settings")
let pane = URL(fileURLWithPath: "/System/Library/PreferencePanes/Security.prefPane")
let alias = try! pane.bookmarkData(options: .suitableForBookmarkFile, includingResourceValuesForKeys: nil, relativeTo: nil)
try! URL.writeBookmarkData(alias, to: shortcut)
let gear = NSWorkspace.shared.icon(forFile: "/System/Applications/System Settings.app")
gear.size = NSSize(width: 256, height: 256)
NSWorkspace.shared.setIcon(gear, forFile: shortcut.path, options: [])
