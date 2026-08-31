import Foundation
import SwiftHelper

protocol ServiceProtocol {
    func start()
}

enum State {
    case ready
    case running
}

struct Config {
    let id: String
}

class SampleService: ServiceProtocol {
    let config: Config

    init(config: Config) {
        self.config = config
    }

    func start() {
        process()
    }

    func process() {
        SwiftHelper.doWork()
    }
}

extension SampleService {
    func extraMethod() {
        start()
    }
}

func globalBootstrap() {
    let service = SampleService(config: Config(id: "1"))
    service.start()
}
