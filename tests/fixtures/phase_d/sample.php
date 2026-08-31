<?php

namespace App\Services;

use App\Utils\Helper;

interface ProcessorInterface {
    public function process(): void;
}

trait LoggerTrait {
    public function logMessage(string $msg): void {
        echo $msg;
    }
}

enum OrderStatus {
    case Pending;
    case Completed;
}

class SampleService implements ProcessorInterface {
    use LoggerTrait;

    private string $id;

    public function __construct(string $id) {
        $this->id = $id;
    }

    public function process(): void {
        $this->logMessage("processing");
        Helper::doWork();
    }
}

function standaloneFunction(): void {
    $service = new SampleService("1");
    $service->process();
}
