// dart format off

// ignore_for_file: avoid_equals_and_hash_code_on_mutable_classes, document_ignores, lines_longer_than_80_chars

part of 'main.dart';

class User extends _User {
  User({
    required this.x,
  });

  factory User.fromJson(Map<String, dynamic> json) {
    return User(
      x: json['x'] == null ? null : Role.values.singleWhere((v) => v.name == json['x'] as String),
    );
  }

  @override
  final Role? x;

  Map<String, dynamic> toJson() => {
    'x': x?.name,
  };

  @override
  String toString() => 'User('
    'x: $x '
    ')';

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    return other is User
      && other.x == x;
  }

  @override
  int get hashCode => Object.hashAll([
    x.hashCode,
  ]);
}
