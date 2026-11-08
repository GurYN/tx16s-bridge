// Draws the app icon and writes assets/AppIcon.icns.
// Usage: swift scripts/make-icon.swift   (from the repository root)
import AppKit
import CoreGraphics

let canvas: CGFloat = 1024

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat(hex >> 16 & 0xff) / 255, green: CGFloat(hex >> 8 & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}

func linearGradient(_ ctx: CGContext, _ colors: [CGColor], from: CGPoint, to: CGPoint) {
    let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                              colors: colors as CFArray, locations: nil)!
    ctx.drawLinearGradient(gradient, start: from, end: to, options: [])
}

func draw(_ ctx: CGContext) {
    // Top-left origin, like the design grid.
    ctx.translateBy(x: 0, y: canvas)
    ctx.scaleBy(x: 1, y: -1)

    // macOS icon tile: 824 pt square on a 1024 canvas, with a soft drop shadow.
    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: 12), blur: 28, color: color(0x000000, 0.35))
    ctx.addPath(tilePath)
    ctx.setFillColor(color(0x161a21))
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(tilePath)
    ctx.clip()
    linearGradient(ctx, [color(0x262d3a), color(0x111419)],
                   from: CGPoint(x: 512, y: 100), to: CGPoint(x: 512, y: 924))
    ctx.restoreGState()
    ctx.addPath(tilePath)
    ctx.setStrokeColor(color(0xffffff, 0.08))
    ctx.setLineWidth(4)
    ctx.strokePath()

    // Signal arcs radiating from the antenna: the radio being streamed over the network.
    let antennaTip = CGPoint(x: 512, y: 430)
    ctx.setLineCap(.round)
    for (index, radius) in [CGFloat(95), 165, 235].enumerated() {
        ctx.addArc(center: antennaTip, radius: radius,
                   startAngle: .pi * 1.27, endAngle: .pi * 1.73, clockwise: false)
        ctx.setStrokeColor(color(0x4f8cff, 1 - CGFloat(index) * 0.28))
        ctx.setLineWidth(36)
        ctx.strokePath()
    }

    let body = CGRect(x: 172, y: 520, width: 680, height: 270)

    // Antenna and shoulder switches sit behind the body's top edge.
    ctx.move(to: CGPoint(x: 512, y: body.minY + 20))
    ctx.addLine(to: antennaTip)
    ctx.setStrokeColor(color(0xb9bec7))
    ctx.setLineWidth(24)
    ctx.strokePath()
    ctx.addEllipse(in: CGRect(x: antennaTip.x - 30, y: antennaTip.y - 30, width: 60, height: 60))
    ctx.setFillColor(color(0x4f8cff))
    ctx.fillPath()
    for x in [CGFloat(262), 762] {
        ctx.move(to: CGPoint(x: x, y: body.minY + 10))
        ctx.addLine(to: CGPoint(x: x - 14, y: body.minY - 38))
        ctx.setStrokeColor(color(0x8b919b))
        ctx.setLineWidth(18)
        ctx.strokePath()
    }

    // Transmitter body.
    let bodyPath = CGPath(roundedRect: body, cornerWidth: 96, cornerHeight: 96, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: 10), blur: 24, color: color(0x000000, 0.45))
    ctx.addPath(bodyPath)
    ctx.setFillColor(color(0xe6e8eb))
    ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(bodyPath)
    ctx.clip()
    linearGradient(ctx, [color(0xf4f5f7), color(0xb4b9c2)],
                   from: CGPoint(x: 512, y: body.minY), to: CGPoint(x: 512, y: body.maxY))
    ctx.restoreGState()

    // Center screen, showing channel bars like the app does.
    let screen = CGRect(x: 412, y: 590, width: 200, height: 130)
    ctx.addPath(CGPath(roundedRect: screen, cornerWidth: 20, cornerHeight: 20, transform: nil))
    ctx.setFillColor(color(0x1c2129))
    ctx.fillPath()
    for (index, width) in [CGFloat(110), 70, 140].enumerated() {
        let bar = CGRect(x: screen.minX + 26, y: screen.minY + 28 + CGFloat(index) * 30,
                         width: width, height: 16)
        ctx.addPath(CGPath(roundedRect: bar, cornerWidth: 8, cornerHeight: 8, transform: nil))
        ctx.setFillColor(color(0x4f8cff, index == 1 ? 0.6 : 1))
        ctx.fillPath()
    }

    // Two gimbals, sticks pushed in different directions.
    for (center, tip) in [(CGPoint(x: 296, y: 655), CGPoint(x: 268, y: 618)),
                          (CGPoint(x: 728, y: 655), CGPoint(x: 764, y: 632))] {
        let ring = CGRect(x: center.x - 86, y: center.y - 86, width: 172, height: 172)
        ctx.addEllipse(in: ring)
        ctx.setFillColor(color(0x1c2129))
        ctx.fillPath()
        ctx.addEllipse(in: ring)
        ctx.setStrokeColor(color(0x8b919b, 0.6))
        ctx.setLineWidth(6)
        ctx.strokePath()
        ctx.addEllipse(in: CGRect(x: center.x - 18, y: center.y - 18, width: 36, height: 36))
        ctx.setFillColor(color(0x8b919b))
        ctx.fillPath()
        ctx.move(to: center)
        ctx.addLine(to: tip)
        ctx.setStrokeColor(color(0x8b919b))
        ctx.setLineWidth(20)
        ctx.strokePath()
        ctx.addEllipse(in: CGRect(x: tip.x - 28, y: tip.y - 28, width: 56, height: 56))
        ctx.setFillColor(color(0x4f8cff))
        ctx.fillPath()
    }
}

func render(size: Int) -> Data {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    ctx.scaleBy(x: CGFloat(size) / canvas, y: CGFloat(size) / canvas)
    draw(ctx)
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    return rep.representation(using: .png, properties: [:])!
}

let fm = FileManager.default
let iconset = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("AppIcon.iconset")
try? fm.removeItem(at: iconset)
try! fm.createDirectory(at: iconset, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    try! render(size: points).write(to: iconset.appendingPathComponent("icon_\(points)x\(points).png"))
    try! render(size: points * 2).write(to: iconset.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
try! render(size: 1024).write(to: URL(fileURLWithPath: "assets/AppIcon.png"))

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", "assets/AppIcon.icns"]
try! iconutil.run()
iconutil.waitUntilExit()
print(iconutil.terminationStatus == 0 ? "Wrote assets/AppIcon.icns" : "iconutil failed")
