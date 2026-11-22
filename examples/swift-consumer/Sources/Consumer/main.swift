import DemoKit

let title = "Ship Rust → Swift 🦀 香港"
precondition(greet(name: "香港 🦀") == "Hello, 香港 🦀!")
for _ in 0..<100 {
    let manager = TaskManager()
    let task = manager.createTask(title: title)
    precondition(task.id == 1 && task.title == title && !task.done)
    let toggled = manager.toggle(task: task)
    precondition(toggled.id == task.id && toggled.title == title && toggled.done)
    precondition(!manager.toggle(task: toggled).done)
}
print("PASS: Swift consumer called Rust; Unicode, records, object creation/drop, and toggles")
