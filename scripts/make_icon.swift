import AppKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

let size: CGFloat = 1024
let colorSpace = CGColorSpaceCreateDeviceRGB()
let ctx = CGContext(
    data: nil,
    width: Int(size),
    height: Int(size),
    bitsPerComponent: 8,
    bytesPerRow: 0,
    space: colorSpace,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
)!
ctx.translateBy(x: 0, y: size)
ctx.scaleBy(x: 1, y: -1)

let inset = size * 0.08
let rect = CGRect(x: inset, y: inset, width: size - inset * 2, height: size - inset * 2)
let radius = rect.width * 0.23
let path = CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
ctx.setFillColor(CGColor(gray: 0.07, alpha: 1))
ctx.addPath(path)
ctx.fillPath()

let ringRect = rect.insetBy(dx: rect.width * 0.22, dy: rect.height * 0.22)
ctx.setStrokeColor(CGColor(gray: 1, alpha: 0.14))
ctx.setLineWidth(size * 0.055)
ctx.strokeEllipse(in: ringRect)

ctx.setStrokeColor(CGColor(red: 0.30, green: 0.86, blue: 0.42, alpha: 1))
ctx.setLineCap(.round)
ctx.addArc(
    center: CGPoint(x: ringRect.midX, y: ringRect.midY),
    radius: ringRect.width / 2,
    startAngle: -.pi / 2,
    endAngle: -.pi / 2 + .pi * 1.35,
    clockwise: false
)
ctx.strokePath()

let image = ctx.makeImage()!
let out = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    .appendingPathComponent("app-icon.png")
let dest = CGImageDestinationCreateWithURL(out as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, image, nil)
CGImageDestinationFinalize(dest)
print(out.path)
