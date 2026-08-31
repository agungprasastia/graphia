const std = @import("std");
const helper = @import("helper.zig");

pub const Status = enum {
    Active,
    Inactive,
};

pub const User = struct {
    id: u32,
    name: []const u8,

    pub fn init(id: u32, name: []const u8) User {
        return User{
            .id = id,
            .name = name,
        };
    }

    pub fn process(self: *User) void {
        self.log();
    }

    pub fn log(self: *User) void {
        _ = self;
    }
};

pub fn runMain() void {
    helper.doWork();
}
