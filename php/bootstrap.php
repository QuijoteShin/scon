<?php
# php/bootstrap.php
# Standalone bootstrap — provides bX\Exception when running outside the kernel

namespace bX {
    if (!class_exists('bX\\Exception')) {
        class Exception extends \Exception {}
    }
}
