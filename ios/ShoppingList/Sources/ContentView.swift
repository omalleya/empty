import SwiftUI

struct ContentView: View {
    @StateObject private var store = ShoppingListStore.shared
    @StateObject private var speech = SpeechRecognizer()
    @State private var newItemText = ""
    @State private var permissionDenied = false

    var body: some View {
        NavigationStack {
            List {
                ForEach(store.items) { item in
                    Button {
                        store.toggle(item)
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: item.isChecked ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(item.isChecked ? .green : .secondary)
                            Text(item.name)
                                .strikethrough(item.isChecked)
                                .foregroundStyle(item.isChecked ? .secondary : .primary)
                        }
                    }
                    .tint(.primary)
                }
                .onDelete(perform: store.remove)
            }
            .overlay {
                if store.items.isEmpty {
                    ContentUnavailableView(
                        "No items",
                        systemImage: "cart",
                        description: Text("Add something below, or tap the mic to dictate.")
                    )
                }
            }
            .navigationTitle("Shopping List")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { EditButton() }
                ToolbarItem(placement: .topBarTrailing) {
                    if store.items.contains(where: \.isChecked) {
                        Button("Clear ✓") { store.clearChecked() }
                    }
                }
            }
            .safeAreaInset(edge: .bottom) { entryBar }
            .alert("Permission needed", isPresented: $permissionDenied) {
                Button("OK", role: .cancel) {}
            } message: {
                Text("Enable Microphone and Speech Recognition in Settings to dictate.")
            }
        }
    }

    private var entryBar: some View {
        HStack(spacing: 12) {
            TextField("Add an item…", text: $newItemText)
                .textFieldStyle(.roundedBorder)
                .onSubmit(add)

            Button(action: toggleDictation) {
                Image(systemName: speech.isRecording ? "mic.fill" : "mic")
                    .font(.title2)
                    .foregroundStyle(speech.isRecording ? .red : Color.accentColor)
                    .symbolEffect(.pulse, isActive: speech.isRecording)
            }
            .accessibilityLabel(speech.isRecording ? "Stop dictation" : "Start dictation")

            Button(action: add) {
                Image(systemName: "plus.circle.fill").font(.title)
            }
            .disabled(newItemText.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding()
        .background(.bar)
        .onChange(of: speech.transcript) { _, newValue in
            // While dictating, mirror the live transcript into the entry field.
            if speech.isRecording { newItemText = newValue }
        }
    }

    private func add() {
        store.add(newItemText)
        newItemText = ""
    }

    private func toggleDictation() {
        if speech.isRecording {
            speech.stop()
            return
        }
        Task {
            guard await speech.requestPermission() else {
                permissionDenied = true
                return
            }
            speech.transcript = ""
            newItemText = ""
            try? speech.start()
        }
    }
}

#Preview {
    ContentView()
}
