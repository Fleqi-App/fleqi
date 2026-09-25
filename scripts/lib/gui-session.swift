// Native UI acceptance needs an unlocked login session. Only emit readiness, not account data.
import CoreGraphics
import Foundation

guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else {
  print("unavailable")
  exit(3)
}
if session["CGSSessionScreenIsLocked"] as? Bool == true {
  print("locked")
  exit(2)
}
print("unlocked")
